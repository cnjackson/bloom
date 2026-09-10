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

use std::sync::Mutex;
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE,
    VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT, VK_OEM_1, VK_OEM_2, VK_OEM_3,
    VK_OEM_4, VK_OEM_5, VK_OEM_6, VK_OEM_7, VK_OEM_COMMA, VK_OEM_MINUS,
    VK_OEM_PERIOD, VK_OEM_PLUS, VK_PRIOR, VK_RETURN,
    VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP, VIRTUAL_KEY,
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
    // HOOK can be null on early events if SetWindowsHookExW failed but
    // the message loop still gets messages - bail safely to avoid UB.
    if HOOK.0.is_null() {
        return unsafe { CallNextHookEx(None, ncode, wparam, lparam) };
    }
    if ncode < 0 {
        return unsafe { CallNextHookEx(Some(HOOK), ncode, wparam, lparam) };
    }
    let st: &KBDLLHOOKSTRUCT = std::mem::transmute(lparam.0 as *const KBDLLHOOKSTRUCT);
    // Pass through our own synthetic input (re-processing would
    // corrupt the buffer and re-trigger).
    if (st.flags & LLKHF_INJECTED).0 != 0 {
        return unsafe { CallNextHookEx(Some(HOOK), ncode, wparam, lparam) };
    }
    if wparam.0 as u32 == 0x0100 || wparam.0 as u32 == 0x0104 {
            // Detect Shift state for shifted chars like '!' (Shift+1) so
            // triggers like '!!critique' work. Modifier-only keypresses
            // (Shift itself) are handled in handle_key and return Ignore.
            let shift_down = unsafe { (GetAsyncKeyState(0x10) as i16) < 0 };
            handle_key(VIRTUAL_KEY(st.vkCode as u16), shift_down);
        }
    unsafe { CallNextHookEx(Some(HOOK), ncode, wparam, lparam) }
}

fn handle_key(vk: VIRTUAL_KEY, shift_down: bool) {
    match vk_action(vk, shift_down) {
        BufAction::Char(c) => {
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
                                    // The user may have typed extra whitespace before the
                                    // boundary (e.g. "answer  short ") — and may have a
                                    // sentence prefix before the trigger too (e.g.
                                    // "have an icecream. answer short "). Only the
                                    // *trigger span* at the end of the buffer should be
                                    // erased; the prefix stays put.
                                    //
                                    // The trailing whitespace boundary is included in the
                                    // buffer (handle_key pushes it before checking), so
                                    // the backspace count is
                                    //     (buffer_len − trigger_start_in_buffer).
                                    // The +1 we had before was wrong: rfind returns the
                                    // start offset, so backspacing (pos − 0) + 1 was
                                    // actually backspacing the whole buffer again.
                                    let (typed_len, n) = BUFFER.with(|b| {
                                        let lock = b.borrow();
                                        let buf = lock.as_ref().unwrap();
                                        let inner = buf.lock().unwrap();
                                        let len = inner.len();
                                        // `rule.trigger` is the normalized trigger
                                        // (e.g. "answer short", 12 chars). Find its
                                        // start offset back-to-front in the buffer.
                                        // The buffer at match time includes the
                                        // trailing whitespace boundary, so the match
                                        // is always at len - trig_len - 1 -ish unless
                                        // there's a sentence prefix.
                                        let trig_lc = rule.trigger.to_ascii_lowercase();
                                        let buf_lc: String = inner.iter().collect::<String>().to_ascii_lowercase();
                                        // Default to 0 start offset (=no prefix) only if
                                        // somehow the trigger isn't in the buffer. The
                                        // matcher wouldn't have fired in that case, so
                                        // this is just defensive.
                                        let trigger_start = buf_lc.rfind(&trig_lc).unwrap_or(0);
                                        let n = len.saturating_sub(trigger_start);
                                        (len, n)
                                    });
                                    hook_debug(&format!(
                                        "MATCH: {} -> {:?} (typed_len={typed_len} n={n})",
                                        rule.trigger, rule.replacement
                                    ));
                                    expand(&rule, n);
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
        BufAction::Backspace => {
            BUFFER.with(|b| {
                let mut lock = b.borrow_mut();
                let buf = lock.as_mut().unwrap();
                let mut inner = buf.lock().unwrap();
                inner.pop();
            });
        }
        // Esc, Tab, arrows, nav, modifiers, function keys: don't touch the
        // buffer at all. Mac-style Text Replacement tolerates these too.
        BufAction::Ignore => {}
    }
}

/// What we do with a key.
#[derive(Clone, Copy)]
enum BufAction {
    /// A regular character: 'a'-'z', '0'-'9', space, return.
    Char(char),
    /// Backspace: pop the buffer by one character (no auto-expand).
    Backspace,
    /// Modifier / navigation / function / escape: leave the buffer alone.
    Ignore,
}

/// Map virtual-key codes to actions (US layout). Letters folded to
/// lowercase in the buffer (matching is case-insensitive).
fn vk_action(vk: VIRTUAL_KEY, shift_down: bool) -> BufAction {
    let v = vk.0;
    // Modifiers, navigation, function, edit keys we don't act on. These
    // arrive via `handle_key` but should never reset the user's typing buffer.
    if v == VK_ESCAPE.0
        || v == VK_TAB.0
        || v == VK_LEFT.0
        || v == VK_RIGHT.0
        || v == VK_UP.0
        || v == VK_DOWN.0
        || v == VK_HOME.0
        || v == VK_END.0
        || v == VK_PRIOR.0
        || v == VK_NEXT.0
        || v == VK_INSERT.0
        || v == VK_DELETE.0
        || v == 0x10
        || v == 0x11
        || v == 0x12
        || v == 0x5B
        || (0x70..=0x7B).contains(&v)
    {
        return BufAction::Ignore;
    }
    if v == VK_BACK.0 as u16 {
        return BufAction::Backspace;
    }
    let c: char = match v {
        0x41..=0x5A => ((v - 0x41) + b'a' as u16) as u8 as char,
        // Digit row: 1..=0 with Shift gives us the symbols needed for
        // !!-prefix triggers (Shift+1 = '!'). Without the shift branch
        // triggers like '!!critique' or '/perf' would never fire
        // because the buffer accumulates unshifted digits/symbols.
        0x30..=0x39 => {
            if shift_down {
                match v {
                    0x31 => '!',
                    0x32 => '@',
                    0x33 => '#',
                    0x34 => '$',
                    0x35 => '%',
                    0x36 => '^',
                    0x37 => '&',
                    0x38 => '*',
                    0x39 => '(',
                    // Shift+0 = ')' on US layout; 0 is outside 0x30..=0x39
                    _ => v as u8 as char,
                }
            } else {
                v as u8 as char
            }
        }
        x if x == VK_SPACE.0 => ' ',
        x if x == VK_RETURN.0 => '\n',
        x if x == VK_OEM_1.0 => if shift_down { ':' } else { ';' }, // ;:
        x if x == VK_OEM_2.0 => if shift_down { '?' } else { '/' }, // /?
        x if x == VK_OEM_3.0 => if shift_down { '~' } else { '`' }, // `~
        x if x == VK_OEM_4.0 => if shift_down { '{' } else { '[' }, // [{
        x if x == VK_OEM_5.0 => if shift_down { '|' } else { '\\' }, // \|
        x if x == VK_OEM_6.0 => if shift_down { '}' } else { ']' }, // ]}
        x if x == VK_OEM_7.0 => if shift_down { '"' } else { '\'' }, // '"
        x if x == VK_OEM_MINUS.0 => if shift_down { '_' } else { '-' }, // -_
        x if x == VK_OEM_PLUS.0 => if shift_down { '+' } else { '=' }, // =+
        x if x == VK_OEM_COMMA.0 => if shift_down { '<' } else { ',' }, // ,<
        x if x == VK_OEM_PERIOD.0 => if shift_down { '>' } else { '.' }, // .>
        // OEM keys, IME, function keys, and friends: ignore (a trigger
        // attached to a Tab or arrow shouldn't expand).
        _ => return BufAction::Ignore,
    };
    BufAction::Char(c)
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
        // Compare case-insensitively — Windows exes are normalized to lowercase
        // by resolve_focus, but rule-authored values may have any case.
                if let Some(scope) = scoped_to {
                    if exe.is_empty() || !scope.eq_ignore_ascii_case(exe) {
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
/// `typed_len` is the number of characters in the buffer at the moment
/// the match fired — that's what we need to backspace, NOT the trigger's
/// `chars().count()` (which is the *normalized* length). With extra
/// whitespace like `"answer  short "` this differs by 1+ characters.
fn expand(rule: &Rule, typed_len: usize) {
    // Backspaces run synchronously on the hook thread (SendInput is
    // fast, and the focused app needs the backspace bytes NOW). The
    // clipboard put + Ctrl+V + 120ms wait + restore run on a worker
    // thread so the hook doesn't stall while the paste lands.
    //
    // Trade-off: keystrokes that arrive during the 120ms paste window
    // continue into the buffer. If the user pauses-and-continues, the
    // next match might fire with stale state. Acceptable for v0.1; a
    // future flush-pending-buffer pass would replace this.
    let n = typed_len;
    BUFFER.with(|b| {
        if let Some(buf) = b.borrow().as_ref() {
            buf.lock().unwrap().clear();
        }
    });
    let ok = unsafe { send_backspaces(n) };
    if !ok {
        hook_debug("backspace injection failed");
    }

    // Hand off the rest to a worker so the hook thread returns to
    // pumping messages immediately.
    let replacement = rule.replacement.clone();
    std::thread::spawn(move || {
        paste_back(replacement);
    });
}

fn paste_back(replacement: String) {
    let mut clip = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("bloom: clipboard unavailable: {e}");
            return;
        }
    };
    let original = clip.get_text().ok();
    if clip.set_text(&replacement).is_err() {
        eprintln!("bloom: clipboard set failed");
        return;
    }
    unsafe { send_ctrl_v() };
    std::thread::sleep(Duration::from_millis(120));
    if let Some(old) = original {
        let _ = clip.set_text(&old);
    }
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
