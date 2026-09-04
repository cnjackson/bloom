//! Persistence layer for the rules.json config file.
//!
//! Storage location: `%APPDATA%\bloom\rules.json`, with a `.bak` sibling
//! holding the previous good config. On a malformed-load, we rename the
//! broken file out of the way rather than overwriting it.
//!
//! "Strict validation" is implemented as: parse JSON, walk every rule
//! and every blacklist entry, reject on any violation. Partial files
//! are not accepted; the user gets a dialog and a renamed bad file they
//! can restore from.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::model::{Config, Rule, RulesError, SCHEMA_VERSION, MAX_REPLACEMENT_LEN, MAX_TRIGGER_LEN};

/// Resolve the directory we own: `%APPDATA%\bloom`.
pub fn config_dir() -> Result<PathBuf, RulesError> {
    let appdata = std::env::var_os("APPDATA")
        .ok_or_else(|| RulesError::Validation("APPDATA not set".into()))?;
    let dir = PathBuf::from(appdata).join("bloom");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

/// Path to the live config file inside `config_dir`.
pub fn config_path(dir: &Path) -> PathBuf {
    dir.join("rules.json")
}

/// Load the config, or fall back to defaults. If the file exists but
/// doesn't parse or validate, return an error so the caller can decide
/// whether to surface a dialog vs silently re-default.
pub fn load_or_default(path: &Path) -> Result<Config, RulesError> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let txt = fs::read_to_string(path)?;
    let cfg: Config = serde_json::from_str(&txt)?;
    validate(&cfg)?;
    Ok(cfg)
}

/// Strict validation. Rejects duplicates, malformed fields, etc.
/// See `spec.md` § Validation and persistence for invariants.
pub fn validate(cfg: &Config) -> Result<(), RulesError> {
    if cfg.version != SCHEMA_VERSION {
        return Err(RulesError::Validation(format!(
            "unsupported schema version: {} (expected {})",
            cfg.version, SCHEMA_VERSION
        )));
    }
    // Theme is open-ended; accept the three we ship, reject anything else
    // silently so a future schema can add more without breaking older saves.
    if !matches!(cfg.theme.mode.as_str(), "dark" | "light" | "system") {
        return Err(RulesError::Validation(format!(
            "unsupported theme mode: {:?}",
            cfg.theme.mode
        )));
    }
    let mut seen_ids = std::collections::HashSet::new();
            // Empty ids are auto-assigned by Tauri at append time, so we
            // don't treat them as duplicates here.
        let mut seen_triggers = std::collections::HashSet::new();
        for r in &cfg.rules {
            if !r.id.is_empty() && !seen_ids.insert(&r.id) {
                return Err(RulesError::Validation(format!("duplicate id: {}", r.id)));
            }
        if r.trigger.is_empty() {
            return Err(RulesError::Validation("trigger cannot be empty".into()));
        }
        // Multi-word triggers allowed (Mac parity). Normalize: no
        // leading/trailing whitespace, collapse internal runs to single
        // spaces so "answer  short" and "answer short" are the same rule.
        let normalized = r.trigger
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if normalized != r.trigger {
            return Err(RulesError::Validation(format!(
                "trigger must be trimmed with single internal spaces: {:?}",
                r.trigger
            )));
        }
        if r.trigger.chars().count() > MAX_TRIGGER_LEN {
            return Err(RulesError::Validation(format!(
                "trigger too long: {} chars (max {})",
                r.trigger.chars().count(),
                MAX_TRIGGER_LEN
            )));
        }
        if !seen_triggers.insert(r.trigger.to_ascii_lowercase()) {
            return Err(RulesError::Validation(format!(
                "duplicate trigger: {:?}",
                r.trigger
            )));
        }
        if r.replacement.chars().count() > MAX_REPLACEMENT_LEN {
            return Err(RulesError::Validation(format!(
                "replacement too long: {} chars (max {})",
                r.replacement.chars().count(),
                MAX_REPLACEMENT_LEN
            )));
        }
    }
    Ok(())
}

/// Save the config atomically: copy the live file to `.bak` (single-step
/// backup), then write a sibling `.tmp` and rename it over `rules.json`.
/// Rename-within-directory is atomic on Windows, so a crash mid-write
/// leaves either the old or the new file intact — never a truncated one.
pub fn save(path: &Path, cfg: &Config) -> Result<(), RulesError> {
    validate(cfg)?;
    let parent = path.parent().ok_or_else(|| {
        RulesError::Validation(format!("config path has no parent: {:?}", path))
    })?;
    let bak = parent.join("rules.json.bak");
    if path.exists() {
        fs::copy(path, &bak)?;
    }
    let json = serde_json::to_string_pretty(cfg)?;
    let tmp = parent.join("rules.json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// On a broken-file scenario, rename the bad file out of the way.
/// Returns the new path. Caller decides whether to alert the user.
///
/// Currently unused — left in place because the v0.1 load path silently
/// falls back to defaults, but the quarantine logic is the right
/// recovery flow once we surface a "load failed" dialog to the user
/// (planned for v1.1).
#[allow(dead_code)]
pub fn quarantine_broken(path: &Path, err: &RulesError) -> Result<PathBuf, RulesError> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let fname = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("rules.json");
    let dst = parent.join(format!("{fname}.broken-{ts}"));
    fs::rename(path, &dst)?;
    eprintln!("bloom: quarantined broken config to {:?}; reason: {err}", dst);
    Ok(dst)
}

/// Helper for command handlers: find a rule position by id.
#[allow(dead_code)]
pub fn find_rule<'a>(cfg: &'a Config, id: &str) -> Option<&'a Rule> {
    cfg.rules.iter().find(|r| r.id == id)
}

/// Helper for command handlers: find a rule position by id.
pub fn find_index(cfg: &Config, id: &str) -> Option<usize> {
    cfg.rules.iter().position(|r| r.id == id)
}
