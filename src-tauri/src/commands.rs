//! Tauri command implementations exposed to the React UI.
//!
//! Each command takes the application state, mutates it in place, and
//! returns the full `Config` so the UI has one source of truth on every
//! round-trip.
//!
//! Errors are returned as `String` so the JS side can `.alert()` them
//! directly; we lose type fidelity but gain uniform handling.

use std::{fs, path::PathBuf};

use chrono::Utc;
use tauri::{AppHandle, Manager, State};

use crate::model::{Config, Rule, MAX_REPLACEMENT_LEN, MAX_TRIGGER_LEN};
use crate::{AppState, store};

/// Generate a fresh ULID-style id: 10-char millisecond timestamp prefix
/// (lexically sortable) + 16 random chars from the OS CSPRNG. Collision
/// probability is negligible; no per-process counter needed.
fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // Crockford base32 alphabet minus I/L/O/U to keep ids readable.
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut prefix = String::with_capacity(10);
    let mut v = ms;
    for _ in 0..10 {
        prefix.push(ALPHABET[(v & 0x1F) as usize] as char);
        v >>= 5;
    }
    let mut id: String = prefix.chars().rev().collect();
    // 80 bits of OS entropy for the tail.
    let mut bytes = [0u8; 10];
    getrandom::getrandom(&mut bytes).expect("CSPRNG unavailable");
    let mut acc: u64 = 0;
    let mut acc_bits = 0u32;
    for &b in &bytes {
        acc = (acc << 8) | b as u64;
        acc_bits += 8;
        while acc_bits >= 5 {
            acc_bits -= 5;
            id.push(ALPHABET[((acc >> acc_bits) & 0x1F) as usize] as char);
        }
    }
    id
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Config {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
pub fn add_rule(state: State<'_, AppState>, mut rule: Rule) -> Result<Config, String> {
    if rule.id.is_empty() {
        rule.id = new_id();
    }
    if rule.created_at.is_empty() {
        rule.created_at = Utc::now().to_rfc3339();
    }
    validate_rule_fields(&rule)?;
    let mut cfg = state.config.lock().unwrap();
    if cfg.rules.iter().any(|r| r.trigger.eq_ignore_ascii_case(&rule.trigger)) {
        return Err(format!("duplicate trigger: {:?}", rule.trigger));
    }
    cfg.rules.push(rule);
    Ok(cfg.clone())
}

#[tauri::command]
pub fn update_rule(
    state: State<'_, AppState>,
    id: String,
    mut rule: Rule,
) -> Result<Config, String> {
    if rule.id != id {
        return Err("rule id mismatch".into());
    }
    if rule.created_at.is_empty() {
        rule.created_at = Utc::now().to_rfc3339();
    }
    validate_rule_fields(&rule)?;
    let mut cfg = state.config.lock().unwrap();
    let pos = store::find_index(&cfg, &id).ok_or_else(|| err(format!("rule not found: {id}")))?;
    if cfg.rules.iter().enumerate().any(|(i, r)| {
        i != pos && r.trigger.eq_ignore_ascii_case(&rule.trigger)
    }) {
        return Err(format!("duplicate trigger: {:?}", rule.trigger));
    }
    cfg.rules[pos] = rule;
    Ok(cfg.clone())
}

#[tauri::command]
pub fn delete_rule(state: State<'_, AppState>, id: String) -> Result<Config, String> {
    let mut cfg = state.config.lock().unwrap();
    let pos = store::find_index(&cfg, &id).ok_or_else(|| err(format!("rule not found: {id}")))?;
    cfg.rules.remove(pos);
    Ok(cfg.clone())
}

#[tauri::command]
pub fn toggle_rule(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<Config, String> {
    let mut cfg = state.config.lock().unwrap();
    let pos = store::find_index(&cfg, &id).ok_or_else(|| err(format!("rule not found: {id}")))?;
    cfg.rules[pos].enabled = enabled;
    Ok(cfg.clone())
}

/// Persist the in-memory config to disk. Called whenever the user hits
/// Save, or implicitly after every mutation in v2 (currently explicit).
#[tauri::command]
pub fn save_all(
    app: AppHandle,
    state: State<'_, AppState>,
    start_with_windows: bool,
    blacklist: Vec<String>,
) -> Result<Config, String> {
    let mut cfg = state.config.lock().unwrap();
    cfg.start_with_windows = start_with_windows;
    cfg.blacklist = blacklist;
    store::save(&state.config_path, &cfg).map_err(err)?;
    apply_autostart(&app, start_with_windows).map_err(err)?;
    Ok(cfg.clone())
}

#[tauri::command]
pub fn import_json(state: State<'_, AppState>, path: String) -> Result<Config, String> {
    let txt = fs::read_to_string(&path).map_err(err)?;
    let cfg: Config = serde_json::from_str(&txt).map_err(err)?;
    store::validate(&cfg).map_err(err)?;
    let mut current = state.config.lock().unwrap();
    *current = cfg.clone();
    Ok(cfg)
}

#[tauri::command]
pub fn export_json(
    state: State<'_, AppState>,
    app: AppHandle,
    path: Option<String>,
) -> Result<String, String> {
    let target = match path {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => app
            .path()
            .download_dir()
            .or_else(|_| app.path().home_dir())
            .map_err(err)?
            .join("bloom-rules.json"),
    };
    let cfg = state.config.lock().unwrap().clone();
    let json = serde_json::to_string_pretty(&cfg).map_err(err)?;
    fs::write(&target, json).map_err(err)?;
    Ok(target.to_string_lossy().into_owned())
}

// ---------- helpers ----------

fn validate_rule_fields(rule: &Rule) -> Result<(), String> {
    if rule.id.is_empty() {
        return Err("rule id cannot be empty".into());
    }
    if rule.trigger.is_empty() {
        return Err("trigger cannot be empty".into());
    }
    if rule.trigger.chars().any(|c| c.is_whitespace()) {
        return Err("trigger cannot contain whitespace".into());
    }
    if rule.trigger.chars().count() > MAX_TRIGGER_LEN {
        return Err(format!("trigger too long (max {})", MAX_TRIGGER_LEN));
    }
    if rule.replacement.chars().count() > MAX_REPLACEMENT_LEN {
        return Err(format!("replacement too long (max {})", MAX_REPLACEMENT_LEN));
    }
    Ok(())
}

fn apply_autostart(app: &AppHandle, enable: bool) -> tauri::Result<()> {
    use tauri_plugin_autostart::ManagerExt;
    let m = app.autolaunch();
    if enable {
        m.enable().ok();
    } else {
        m.disable().ok();
    }
    Ok(())
}
