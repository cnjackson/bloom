//! Global keyboard hook (v1.1).
//!
//! rdev `grab` intercepts keystrokes system-wide BEFORE the focused app
//! sees them. Behaviour:
//! - typeable chars are buffered (last 64)
//! - on space/enter: if the buffer ends with an enabled rule's trigger
//!   (Mac-parity: case-insensitive, token boundary), erase the trigger
//!   with synthetic backspaces and paste the replacement via
//!   clipboard+Ctrl+V (clipboard restored afterwards)
//! - replacement's first letter copies the trigger's case (Apple rule)
//!
//! v1.1 simplifications vs spec (documented in architecture.md):
//! - clipboard path for ALL expansions (no SendInput-per-char short path)
//! - no password-field / blacklist / Secure Desktop gates yet — next slice

use rdev::{grab, Event, EventType, Key};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::Manager;

use crate::model::Rule;
use crate::AppState;

/// True while WE are synthesizing input (backspaces, Ctrl+V) so the hook
/// passes our own events through without re-processing them.
static INJECTING: AtomicBool = AtomicBool::new(false);

const BUF_MAX: usize = 64;

/// Spawn the grab thread. Blocks forever inside the thread; call once at
/// setup. `app` gives access to live rules via AppState.
pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        // rdev::grab demands Fn, not FnMut - buffer lives behind a Mutex.
        let buf: std::sync::Mutex<Vec<char>> = std::sync::Mutex::new(Vec::with_capacity(BUF_MAX));
        let callback = move |event: Event| -> Option<Event> {
            if INJECTING.load(Ordering::SeqCst) {
                return Some(event); // our own synthetic input: pass through
            }
            if let EventType::KeyPress(key) = event.event_type {
                match key_char(&key) {
                    Some(' ') | Some('\n') => {
                        let mut b = buf.lock().unwrap();
                        let text: String = b.iter().collect();
                        b.clear(); // whitespace is always a boundary
                        let rule = {
                            let state = app.state::<AppState>();
                            let cfg = state.config.lock().unwrap();
                            match_trigger(&text, &cfg.rules)
                        };
                        if let Some(rule) = rule {
                            INJECTING.store(true, Ordering::SeqCst);
                            expand(&text, &rule);
                            INJECTING.store(false, Ordering::SeqCst);
                        }
                    }
                    Some(c) => {
                        let mut b = buf.lock().unwrap();
                        b.push(c);
                        if b.len() > BUF_MAX {
                            b.remove(0);
                        }
                    }
                    None => {
                        // non-typeable key (arrows, modifiers, F-keys):
                        // reset — a trigger glued to an unknown key is not
                        // a trigger the user meant to fire
                        buf.lock().unwrap().clear();
                    }
                }
            }
            Some(event)
        };
        if let Err(e) = grab(callback) {
            eprintln!("bloom: keyboard hook failed: {e:?}");
        }
    });
}

/// Map rdev virtual keys to the chars we care about (letters, digits,
/// space, enter). Everything else -> None.
fn key_char(key: &Key) -> Option<char> {
    use Key::*;
    Some(match key {
        KeyA => 'a', KeyB => 'b', KeyC => 'c', KeyD => 'd', KeyE => 'e',
        KeyF => 'f', KeyG => 'g', KeyH => 'h', KeyI => 'i', KeyJ => 'j',
        KeyK => 'k', KeyL => 'l', KeyM => 'm', KeyN => 'n', KeyO => 'o',
        KeyP => 'p', KeyQ => 'q', KeyR => 'r', KeyS => 's', KeyT => 't',
        KeyU => 'u', KeyV => 'v', KeyW => 'w', KeyX => 'x', KeyY => 'y',
        KeyZ => 'z',
        Num0 => '0', Num1 => '1', Num2 => '2', Num3 => '3', Num4 => '4',
        Num5 => '5', Num6 => '6', Num7 => '7', Num8 => '8', Num9 => '9',
        Space => ' ',
        Return => '\n',
        _ => return None,
    })
}

/// First-match (declared order) trigger ending at the buffer boundary,
/// case-insensitive, token-boundary-checked. Mirrors the tested Python
/// matcher exactly.
fn match_trigger(buffer: &str, rules: &[Rule]) -> Option<Rule> {
    if buffer.is_empty() {
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
        if buffer.len() >= trig.len()
            && buffer[buffer.len() - trig.len()..].eq_ignore_ascii_case(&trig)
        {
            let before = &buffer[..buffer.len() - trig.len()];
            if before.is_empty() || before.ends_with(char::is_whitespace) {
                return Some(r.clone());
            }
        }
    }
    None
}

/// Erase the typed trigger, paste the replacement (case-matched),
/// restore the clipboard.
fn expand(typed_text: &str, rule: &Rule) {
    let n = rule.trigger.chars().count();
    let replacement = apply_case(rule.replacement.replace("\\\n", "\n"), typed_text);

    // 1) erase the trigger with synthetic backspaces
    if simulate_n(Key::Backspace, n).is_err() {
        eprintln!("bloom: backspace injection failed");
        return;
    }

    // 2) clipboard swap + Ctrl+V
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
    let _ = press_combo(Key::ControlLeft, Key::KeyV);
    std::thread::sleep(Duration::from_millis(120));
    if let Some(old) = original {
        let _ = clip.set_text(&old); // restore user's clipboard
    }
}

/// Apple auto-capitalize: replacement's first letter takes the case of
/// the typed trigger's first letter.
fn apply_case(mut replacement: String, typed: &str) -> String {
    let typed_first = typed.chars().next().unwrap_or('x');
    let mut out = String::with_capacity(replacement.len());
    let mut chars = replacement.chars();
    if let Some(first) = chars.next() {
        if typed_first.is_uppercase() {
            out.extend(first.to_uppercase());
        } else {
            out.extend(first.to_lowercase());
        }
        out.push_str(chars.as_str());
    }
    replacement = out;
    replacement
}

fn simulate_n(key: Key, n: usize) -> Result<(), rdev::SimulateError> {
    for _ in 0..n {
        rdev::simulate(&EventType::KeyPress(key))?;
        rdev::simulate(&EventType::KeyRelease(key))?;
    }
    Ok(())
}

fn press_combo(modifier: Key, key: Key) -> Result<(), rdev::SimulateError> {
    rdev::simulate(&EventType::KeyPress(modifier))?;
    rdev::simulate(&EventType::KeyPress(key))?;
    rdev::simulate(&EventType::KeyRelease(key))?;
    rdev::simulate(&EventType::KeyRelease(modifier))?;
    Ok(())
}
