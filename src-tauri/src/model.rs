//! Serde types matching `spec.md` § "v1 data model" (RULES.md).
//! Schema version is bumped only on breaking changes.
//!
//! ULIDs (sortable, 26-char, lexically ordered by creation time) are used
//! for rule ids so the JSON file presents in chronological order without
//! sorting.

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_TRIGGER_LEN: usize = 32;
pub const MAX_REPLACEMENT_LEN: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rule {
    /// ULID; stable across edits.
    pub id: String,
    pub trigger: String,
    pub replacement: String,
    pub enabled: bool,
    /// ISO-8601 timestamp; set on first write, never modified.
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Theme {
    /// "dark" | "light" | "system". The UI layer also accepts "system" and
    /// follows the OS appearance. Currently only "dark" + "light" are
    /// styled; "system" maps to dark until light tokens are wired into
    /// system appearance listeners.
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub version: u32,
    pub start_with_windows: bool,
    /// Exe filenames (case-insensitive on Windows). Common password
    /// managers ship by default; user editable.
    pub blacklist: Vec<String>,
    /// Optional global whitelist. None = expand everywhere non-blacklisted.
    /// Some(list) = only expand when the focused app's exe is in the list.
    /// Per-rule app scope uses the trigger prefix `app.exe:shortcut`;
    /// this list is an additional app-wide filter.
    #[serde(default)]
    pub scoped_to: Option<Vec<String>>,
    #[serde(default = "default_theme")]
        pub theme: Theme,
        /// Show the in-page debug log overlay. Off by default; user opt-in via
        /// the Settings modal. When on, app.js console.log/warn calls surface
        /// in a fixed-position <pre> at the bottom-right of the rules window.
        #[serde(default)]
        pub show_debug_log: bool,
        pub rules: Vec<Rule>,
    }

    fn default_theme() -> Theme {
        Theme { mode: "dark".into() }
    }

    impl Default for Config {
        fn default() -> Self {
            Self {
                version: SCHEMA_VERSION,
                start_with_windows: true,
                blacklist: vec![
                    "KeePass.exe".into(),
                    "1password.exe".into(),
                    "Bitwarden.exe".into(),
                ],
                scoped_to: None,
                theme: default_theme(),
                show_debug_log: false,
                rules: vec![],
            }
        }
    }

#[derive(Debug, thiserror::Error)]
pub enum RulesError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("rule validation: {0}")]
    Validation(String),
}

impl From<RulesError> for String {
    fn from(e: RulesError) -> Self { e.to_string() }
}
