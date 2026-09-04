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

use crate::model::{Config, Rule, Theme, MAX_REPLACEMENT_LEN, MAX_TRIGGER_LEN};
use crate::{AppState, store};

/// Cheap, serialized-to-frontend metadata used by the About panel.
#[derive(serde::Serialize)]
pub struct AppMeta {
    pub install_dir: String,
    pub config_path: String,
}

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
    // Always regenerate server-side: the client's draft id is a UI-only
    // placeholder ("__new_N") and must never be persisted. Also keeps the
    // ULID sort-by-creation-time property intact.
    rule.id = new_id();
    if rule.created_at.is_empty() {
        rule.created_at = Utc::now().to_rfc3339();
    }
    validate_rule_fields(&mut rule)?;
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
    validate_rule_fields(&mut rule)?;
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

/// Persist the in-memory config to disk. Called whenever the user hits
/// Save, or implicitly after every mutation in v2 (currently explicit).
#[tauri::command]
pub fn save_all(
    app: AppHandle,
    state: State<'_, AppState>,
    start_with_windows: bool,
    blacklist: Vec<String>,
    scoped_to: Option<Vec<String>>,
    theme: Option<Theme>,
    show_debug_log: bool,
) -> Result<Config, String> {
    let mut cfg = state.config.lock().unwrap();
    cfg.start_with_windows = start_with_windows;
    cfg.blacklist = blacklist;
    cfg.scoped_to = scoped_to;
    cfg.show_debug_log = show_debug_log;
    if let Some(t) = theme {
        cfg.theme = t;
    }
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

/// Merge import: read an exported rules.json from disk, validate it, then
/// apply a per-trigger decision map to either skip or overwrite conflicts.
/// A conflict is a case-insensitive trigger match against the current
/// rules. Decisions: HashMap<trigger, "skip" | "overwrite">. If a
/// conflict's trigger is missing from decisions, that conflict is
/// skipped by default (the user didn't see it — conservative default).
/// Returns the new Config after the merge.
#[tauri::command]
pub fn merge_import(
    state: State<'_, AppState>,
    path: String,
    decisions: std::collections::HashMap<String, String>,
) -> Result<Config, String> {
    let txt = fs::read_to_string(&path).map_err(err)?;
    let incoming: Config = serde_json::from_str(&txt).map_err(err)?;
    store::validate(&incoming).map_err(err)?;

    let mut current = state.config.lock().unwrap();
    // Build a lowercased index of current rules for fast conflict checks.
    let mut existing_lc: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (i, r) in current.rules.iter().enumerate() {
        existing_lc.insert(r.trigger.to_ascii_lowercase(), i);
    }

    let mut added = 0usize;
    let mut overwritten = 0usize;
    let mut skipped = 0usize;

    for incoming_rule in incoming.rules.into_iter() {
        let key = incoming_rule.trigger.to_ascii_lowercase();
        if let Some(&idx) = existing_lc.get(&key) {
            // Conflict — defer to decisions.
            let decision = decisions.get(&incoming_rule.trigger)
                .or_else(|| decisions.get(&key))
                .map(String::as_str)
                .unwrap_or("skip");
            match decision {
                "overwrite" => {
                    // Re-id the imported rule to preserve its identity but
                    // overwrite the existing entry's content.
                    let mut new_rule = incoming_rule;
                    new_rule.id = current.rules[idx].id.clone();
                    new_rule.created_at = current.rules[idx].created_at.clone();
                    current.rules[idx] = new_rule;
                    overwritten += 1;
                }
                _ => {
                    skipped += 1;
                }
            }
        } else {
            // No conflict; append as a fresh rule.
            let mut new_rule = incoming_rule;
            new_rule.id = new_id();
            if new_rule.created_at.is_empty() {
                new_rule.created_at = Utc::now().to_rfc3339();
            }
            current.rules.push(new_rule);
            existing_lc.insert(key, current.rules.len() - 1);
            added += 1;
        }
    }

    eprintln!(
        "bloom: merge_import added={} overwritten={} skipped={}",
        added, overwritten, skipped
    );
    Ok(current.clone())
}

/// Return install-side paths used by the About panel. Cheap (no I/O).
#[tauri::command]
pub fn get_app_meta(state: State<'_, AppState>) -> AppMeta {
    // Best-effort install location: the parent directory of bloom.exe when
    // installed per-user to %LOCALAPPDATA%\Programs\Bloom.
    let install_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    AppMeta {
        install_dir,
        config_path: state.config_path.to_string_lossy().into_owned(),
    }
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

fn validate_rule_fields(rule: &mut Rule) -> Result<(), String> {
    // Normalize: trim + collapse internal whitespace (Mac-parity
    // multi-word triggers; "answer  short" == "answer short").
    rule.trigger = rule.trigger.split_whitespace().collect::<Vec<_>>().join(" ");
    if rule.trigger.is_empty() {
        return Err("trigger cannot be empty".into());
    }
    // Multi-word triggers are allowed (macOS Text Replacement parity).
    // Only trim-checked non-empty + length-capped; matching happens on
    // token boundaries in the hook (v1.1).
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
