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
pub struct Config {
    pub version: u32,
    pub start_with_windows: bool,
    /// Exe filenames (case-insensitive on Windows). Common password
    /// managers ship by default; user editable.
    pub blacklist: Vec<String>,
    pub rules: Vec<Rule>,
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
            rules: vec![],
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RulesError {
    #[error("rule not found: {0}")]
    RuleNotFound(String),
    #[error("rule id mismatch")]
    RuleIdMismatch,
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("rule validation: {0}")]
    Validation(String),
}

impl serde::Serialize for RulesErrorSerde {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.0)
    }
}

/// Wrapper so we can serialize `RulesError` as a string for Tauri's IPC.
/// Tauri commands return `Result<_, String>` in most examples; this
/// matches that convention and removes the need to derive Serialize on
/// the error enum directly (which thiserror::Error doesn't expose by
/// default).
pub struct RulesErrorSerde(String);

impl From<RulesError> for RulesErrorSerde {
    fn from(e: RulesError) -> Self { Self(e.to_string()) }
}

impl serde::Serialize for RulesErrorSerde {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.0)
    }
}

/// Wrapper so we can serialize `RulesError` as a string for Tauri's IPC.
/// Tauri commands return `Result<_, String>` in most examples; this
/// matches that convention and removes the need to derive Serialize on
/// the error enum directly (which thiserror::Error doesn't expose by
/// default).
pub struct RulesErrorSerde(String);

impl From<RulesError> for RulesErrorSerde {
    fn from(e: RulesError) -> Self { Self(e.to_string()) }
}

impl From<RulesError> for String {
    fn from(e: RulesError) -> Self { e.to_string() }
}
