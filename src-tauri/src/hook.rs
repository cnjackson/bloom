//! Global keyboard hook (v1.1) — direct Win32 WH_KEYBOARD_LL.
//!
//! Captures keystrokes system-wide via a low-level keyboard hook + an
//! explicit message pump. Buffer is whitespace-bounded; on match,
//! synthetic backspaces erase the trigger and clipboard+Ctrl+V pastes
//! the replacement (clipboard restored after).
//!
//! Per-rule app scope: triggers can be prefixed `app.exe:shortcut` so
//! the rule fires only when that exe owns the focused window. A
//! global `scoped_to` field on Config adds an app-wide filter on top.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    VK_BACK, VK_CONTROL, VK_RETURN, VK_SPACE, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW,
    GetWindowThreadProcessId, SetWindowsHookExW, TranslateMessage, HHOOK,
    KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::ProcessStatus::K32GetModuleFileNameExW;

use tauri::Manager;

use crate::model::Rule;
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
        // itself; values set in start() (the main thread) leave the
        // hook thread's locals empty -> unwrap() panic at first key.
        let buf: std::sync::Arc<Mutex<Vec<char>>> =
            std::sync::Arc::new(Mutex::new(Vec::with_capacity(BUF_MAX)));
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
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        eprintln!("bloom: hook message loop exited");
    });
}

thread_local! {
    static BUFFER: std::cell::RefCell<Option<std::sync::Arc<Mutex<Vec<char>>>>>
        = const { std::cell::RefCell::new(None) };
    static APP: std::cell::RefCell<Option<tauri::AppHandle>>
        = const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn hook_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode < 0 {
        return CallNextHookEx(Some(HOOK), ncode, wparam, lparam);
    }
    let st: &KBDLLHOOKSTRUCT = std::mem::transmute(lparam.0 as *const KBDLLHOOKSTRUCT);
    // Pass through our own synthetic input (re-processing would
    // corrupt the buffer and re-trigger).
    if (st.flags & LLKHF_INJECTED).0 != 0 {
        return CallNextHookEx(Some(HOOK), ncode, wparam, lparam);
    }
    if wparam.0 as u32 == 0x0100 || wparam.0 as u32 == 0x0104 {
        handle_key(VIRTUAL_KEY(st.vkCode as u16));
    }
    CallNextHookEx(Some(HOOK), ncode, wparam, lparam)
}

fn handle_key(vk: VIRTUAL_KEY) {
    let c = match vk_char(vk) {
        Some(c) => c,
        None => {
            // Non-typeable key: reset buffer (a trigger glued to an
            // unknown key is not a trigger the user meant).
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
            // Whitespace joins the buffer so multi-word triggers
            // accumulate; only successful match / non-typeable resets it.
            let mut inner = buf.lock().unwrap();
            inner.push(c);
            let text: String = inner.iter().collect();
            text
        });
        hook_debug(&format!("boundary: buffer={text:?}"));

        let outcome = APP.with(|a| {
            let cell = a.borrow();
            let app = cell.as_ref().unwrap();
            let state = app.state::<AppState>();
            let cfg = state.config.lock().unwrap();
            let focus = resolve_focus(&cfg.blacklist, &cfg.scoped_to);
            let reason: &'static str = if focus.excluded {
                if cfg.blacklist.iter().any(|b| b.trim().to_ascii_lowercase() == focus.exe) {
                    "blacklisted app"
                } else {
                    "not in scoped_to"
                }
            } else {
                "ok"
            };
            let rule = if !focus.excluded {
                match_trigger(&text, &cfg.rules, &focus.exe)
            } else {
                None
            };
            (rule, reason)
        });

        if outcome.1 != "ok" {
            hook_debug(&format!("skip: {}", outcome.1));
        }
        if let Some(rule) = outcome.0 {
            hook_debug(&format!("MATCH: {} -> {:?}", rule.trigger, rule.replacement));
            expand(&rule);
        } else if outcome.1 == "ok" {
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

/// Map virtual-key codes to chars (US layout). Letters folded to
/// lowercase in the buffer (matching is case-insensitive). The
/// replacement adopts its own configured case — Apple's
/// auto-capitalize rule on the configured text, simplified.
fn vk_char(vk: VIRTUAL_KEY) -> Option<char> {
    Some(match vk.0 {
        0x41..=0x5A => ((vk.0 - 0x41) + b'a' as u16) as u8 as char,
        0x30..=0x39 => vk.0 as u8 as char,
        x if x == VK_SPACE.0 => ' ',
        x if x == VK_RETURN.0 => '\n',
        _ => return None,
    })
}

/// Foreground app lookup + global filter check. exe is lowercase.
struct Focus {
    exe: String,     // empty = unknown
    excluded: bool,  // true if blacklist hit OR scoped_to missed
}

fn resolve_focus(blacklist: &[String], scoped_to: &Option<Vec<String>>) -> Focus {
    let raw_exe: String;
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return Focus { exe: String::new(), excluded: false };
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid == 0 {
            return Focus { exe: String::new(), excluded: false };
        }
        let h_proc = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => return Focus { exe: String::new(), excluded: false },
        };
        let mut buf = [0u16; 260];
        let n = K32GetModuleFileNameExW(Some(h_proc), None, &mut buf);
        if n == 0 {
            return Focus { exe: String::new(), excluded: false };
        }
        let path = String::from_utf16_lossy(&buf[..n as usize]);
        raw_exe = match path.rfind('\\') {
            Some(i) => path[i + 1..].to_string(),
            None => path,
        };
    }
    let exe_lc = raw_exe.to_ascii_lowercase();
    if exe_lc.is_empty() {
        return Focus { exe: String::new(), excluded: false };
    }
    if blacklist.iter().any(|b| b.trim().to_ascii_lowercase() == exe_lc) {
        return Focus { exe: exe_lc, excluded: true };
    }
    if let Some(scope) = scoped_to {
        let in_scope = scope.iter().any(|s| s.trim().to_ascii_lowercase() == exe_lc);
        if !in_scope {
            return Focus { exe: exe_lc, excluded: true };
        }
    }
    Focus { exe: exe_lc, excluded: false }
}

fn match_trigger(buffer: &str, rules: &[Rule], exe: &str) -> Option<Rule> {
    if buffer.is_empty() {
        return None;
    }
    // Whitespace-boundary check: trigger must end at the buffer's last
    // typed character, with whitespace as the boundary.
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
        // Optional app scope: `app.exe:shortcut`. split_once is exact;
        // halves are lowercased to match focused exe.
        let (scoped_to, trig_raw) = match r.trigger.split_once(':') {
            Some((scope, rest)) => (
                Some(scope.trim().to_ascii_lowercase()),
                rest.trim().to_string(),
            ),
            None => (None, r.trigger.clone()),
        };
        let trig = trig_raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if trig.is_empty() {
            continue;
        }
        // Per-rule scope: only fire when this rule's app is the focused one.
        if let Some(scope) = scoped_to {
            if exe.is_empty() || scope != exe {
                continue;
            }
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
    send_key(0x56, false);
    send_key(0x56, true);
    send_key(VK_CONTROL.0 as u16, true);
}

unsafe fn send_key(vk: u16, up: bool) -> bool {
    let input = INPUT {
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
