//! Global keyboard hook (v1.1) — direct Win32 WH_KEYBOARD_LL.
//!
//! rdev's `grab()` silently receives no events on this machine (hook
//! installs, callback never fires). Replaced with a direct
//! SetWindowsHookExW low-level keyboard hook + explicit message pump —
//! the same mechanism AutoHotkey uses. This is the architecture.md
//! documented fallback.
//!
//! Behaviour:
//! - typeable chars buffered (last 64)
//! - on space/enter: buffer matched against enabled rules (first-match,
//!   case-insensitive, token boundary — same semantics as the tested
//!   Python harness)
//! - on match: synthetic backspaces erase the trigger, replacement
//!   pasted via clipboard+Ctrl+V (restored after), INJECTING flag
//!   prevents re-processing our own synthetic events
//!
//! v1.1 simplifications vs spec: clipboard path for all expansions; no
//! password-field / blacklist / Secure-Desktop gates yet (next slice).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use windows::core::w;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, VK_BACK, VK_CONTROL, VK_RETURN, VK_SPACE, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    HHOOK, HOOKPROC, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL,
};

use crate::model::Rule;
use tauri::Manager;

use crate::AppState;

static INJECTING: AtomicBool = AtomicBool::new(false);
static mut HOOK: HHOOK = HHOOK(std::ptr::null_mut());

const BUF_MAX: usize = 64;

fn hook_debug(msg: &str) {
    if std::env::var("BLOOM_HOOK_DEBUG").as_deref() == Ok("1") {
        eprintln!("bloom-hook: {msg}");
    }
}

/// Spawn the hook thread: installs WH_KEYBOARD_LL and pumps messages
/// forever (required for low-level hooks to receive events).
pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || unsafe {
        // Thread-locals are PER-THREAD: initialize on the hook thread
        // itself. Values set in start() belong to the main thread and
        // leave the hook thread's locals empty -> unwrap() panic at
        // first keystroke (crashed as STATUS_STACK_BUFFER_OVERRUN).
        let buf: std::sync::Arc<Mutex<Vec<char>>> = std::sync::Arc::new(Mutex::new(Vec::with_capacity(BUF_MAX)));
        BUFFER.with(|b| *b.borrow_mut() = Some(buf));
        APP.with(|a| *a.borrow_mut() = Some(app));

        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("bloom: SetWindowsHookExW failed: {e}");
                return;
            }
        };
        HOOK = hook;
        eprintln!("bloom: WH_KEYBOARD_LL installed");
        // Message pump — REQUIRED. Low-level hooks are called on the
        // installing thread while it pumps messages.
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        eprintln!("bloom: hook message loop exited");
    });
}

thread_local! {
    static BUFFER: std::cell::RefCell<Option<std::sync::Arc<Mutex<Vec<char>>>>> = const { std::cell::RefCell::new(None) };
    static APP: std::cell::RefCell<Option<tauri::AppHandle>> = const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn hook_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode < 0 {
        return CallNextHookEx(Some(HOOK), ncode, wparam, lparam);
    }
    let st: &KBDLLHOOKSTRUCT = std::mem::transmute(lparam.0 as *const KBDLLHOOKSTRUCT);

    // Pass through our own synthetic input; re-processing it would
    // corrupt the buffer and re-trigger.
    if (st.flags & LLKHF_INJECTED).0 != 0 {
        return CallNextHookEx(Some(HOOK), ncode, wparam, lparam);
    }

    // WM_KEYDOWN (0x0100) / WM_SYSKEYDOWN (0x0104)
    if wparam.0 as u32 == 0x0100 || wparam.0 as u32 == 0x0104 {
        handle_key(VIRTUAL_KEY(st.vkCode as u16));
    }
    CallNextHookEx(Some(HOOK), ncode, wparam, lparam)
}

fn handle_key(vk: VIRTUAL_KEY) {
    let is_shift = unsafe { GetAsyncKeyState(0x10 /* VK_SHIFT */) as u16 & 0x8000 != 0 };
    let c = match vk_char(vk, is_shift) {
        Some(c) => c,
        None => {
            // non-typeable key: reset buffer (trigger glued to an
            // unknown key is not a trigger the user meant to fire)
            BUFFER.with(|b| {
                if let Some(buf) = b.borrow().as_ref() {
                    buf.lock().unwrap().clear();
                }
            });
            return;
        }
    };

    if c == ' ' || c == '\n' {
            let text = BUFFER.with(|b| {
                        let mut lock = b.borrow_mut();
                        let buf = lock.as_mut().unwrap();
                        // Whitespace joins the buffer as a separator char (kept so
                        // match_trigger can do token-boundary checks). The buffer
                        // grows across multi-word triggers; we only clear it on a
                        // successful match below, not on every whitespace.
                        let mut inner = buf.lock().unwrap();
                        inner.push(c);
                        let text: String = inner.iter().collect();
                        text
                    });
                    hook_debug(&format!("boundary: buffer={text:?}"));
        let rule = APP.with(|a| {
            let cell = a.borrow();
            let app = cell.as_ref().unwrap();
            let state = app.state::<AppState>();
            let cfg = state.config.lock().unwrap();
            match_trigger(&text, &cfg.rules)
        });
        if let Some(rule) = rule {
            hook_debug(&format!("MATCH: {} -> {:?}", rule.trigger, rule.replacement));
            expand(&rule);
        } else {
            hook_debug("no match");
        }
    } else {
        BUFFER.with(|b| {
            let mut lock = b.borrow_mut();
            let buf = lock.as_mut().unwrap();
            let mut inner = buf.lock().unwrap();
            inner.push(c);
            if inner.len() > BUF_MAX {
                inner.remove(0);
            }
        });
    }
}

/// Map virtual-key codes to chars (US layout). Uppercase folded to
/// lowercase in the buffer — matching is case-insensitive; the case
/// rule reads the FIRST typed char separately, which loses
/// shift-information here. Documented simplification: expansions adopt
/// the configured replacement's case as typed; auto-capitalize applies
/// only if the whole trigger was typed with Shift held for its first
/// letter — approximated by tracking the last Shift state at first
/// char time. Simplified v1.1: trigger's first letter case is taken
/// from the replacement config itself.
fn vk_char(vk: VIRTUAL_KEY, _shift: bool) -> Option<char> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    Some(match vk.0 {
        0x41..=0x5A => ((vk.0 - 0x41) + b'a' as u16) as u8 as char, // A-Z -> a-z
        0x30..=0x39 => vk.0 as u8 as char,                        // 0-9
        x if x == VK_SPACE.0 => ' ',
        x if x == VK_RETURN.0 => '\n',
        _ => return None,
    })
}

fn match_trigger(buffer: &str, rules: &[Rule]) -> Option<Rule> {
    if buffer.is_empty() {
        return None;
    }
    // The boundary character (whitespace we just typed) is part of the
    // buffer but not part of any trigger. Strip it for matching; require
    // that it BE there (otherwise we matched mid-word).
    if !buffer.ends_with(char::is_whitespace) {
        return None;
    }
    let head = &buffer[..buffer.len() - 1];
    if head.is_empty() {
        return None;
    }
    for r in rules {
        if !r.enabled {
            continue;
        }
        let trig = r.trigger.split_whitespace().collect::<Vec<_>>().join(" ");
        if trig.is_empty() {
            continue;
        }
        if head.len() >= trig.len()
            && head[head.len() - trig.len()..].eq_ignore_ascii_case(&trig)
        {
            let before = &head[..head.len() - trig.len()];
            if before.is_empty() || before.ends_with(char::is_whitespace) {
                return Some(r.clone());
            }
        }
    }
    None
}

/// Erase the typed trigger (synthetic backspaces) and paste the
/// replacement via clipboard + Ctrl+V, restoring the clipboard after.
fn expand(rule: &Rule) {
    let n = rule.trigger.chars().count();
    INJECTING.store(true, Ordering::SeqCst);
    let ok = unsafe { send_backspaces(n) };
    if !ok {
        hook_debug("backspace injection failed");
    }
    // paste
    let mut clip = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("bloom: clipboard unavailable: {e}");
            return;
        }
    };
    let original = clip.get_text().ok();
    if clip.set_text(&rule.replacement).is_err() {
        eprintln!("bloom: clipboard set failed");
        return;
    }
    unsafe { send_ctrl_v() };
    std::thread::sleep(Duration::from_millis(120));
    if let Some(old) = original {
        let _ = clip.set_text(&old);
    }
    INJECTING.store(false, Ordering::SeqCst);
}

unsafe fn send_backspaces(n: usize) -> bool {
    for _ in 0..n {
        if !send_key(VK_BACK.0 as u16, false) {
            return false;
        }
        if !send_key(VK_BACK.0 as u16, true) {
            return false;
        }
    }
    true
}

unsafe fn send_ctrl_v() {
    send_key(VK_CONTROL.0 as u16, false);
    send_key(0x56, false); // 'V'
    send_key(0x56, true);
    send_key(VK_CONTROL.0 as u16, true);
}

unsafe fn send_key(vk: u16, up: bool) -> bool {
    let mut input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    SendInput(&[input], std::mem::size_of::<INPUT>() as i32) == 1
}
