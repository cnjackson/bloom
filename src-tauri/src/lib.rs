//! Bloom — system-wide text replacement for Windows.
//!
//! This is the v0.1 entry point. It contains:
//! - config store (load/save rules.json under %APPDATA%)
//! - Tauri commands backing the rules editor UI
//! - tray icon that opens the rules window
//!
//! The global keyboard hook and paste-back subsystems are not yet wired in
//! (see `architecture.md` § Hook subsystem). Those will land in a
//! follow-up once the install path + UI + persistence are proven end-to-end.

mod commands;
mod hook;
mod model;
mod store;
mod tray;

use tauri::Manager;

use crate::model::Config;
use std::sync::Mutex;

/// Application state shared between commands and the tray.
pub struct AppState {
    pub config: Mutex<Config>,
    /// Where on disk rules.json lives. Resolved on startup.
    pub config_path: std::path::PathBuf,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Resolve %APPDATA%\bloom before we hand control to Tauri.
    let config_dir = store::config_dir().expect("could not resolve APPDATA");
    let config_path = store::config_path(&config_dir);

    // Load the config strictly; bail with a panic + visible message if it
    // is malformed. The Tauri default error handler will show this in the
    // log; user-facing recovery happens via the rules editor once that
    // loads (`load_or_default`).
    let config = store::load_or_default(&config_path).unwrap_or_else(|e| {
        eprintln!("bloom: failed to load config: {e}");
        Config::default()
    });

    let state = AppState {
        config: Mutex::new(config),
        config_path,
    };

    tauri::Builder::default()
        .manage(state)
        // Single-instance lock: a second copy of bloom.exe sends a payload
        // to the running instance (we ignore it) and exits immediately,
        // so we never end up with two `WH_KEYBOARD_LL` hooks competing.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // The user launched a second copy; bring the existing
            // rules window to the front so the click does something
            // useful instead of dying silently.
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        )) // LaunchAgent arg is a no-op on Windows; plugin API requires it.
        .setup(|app| {
            tray::install(app)?;
            hook::start(app.handle().clone());
            // Show the rules window on first process launch. This makes
            // the startup splash visible alongside the rules UI — the
            // splash overlay is removed after its 3-second timer, so the
            // user lands on the rules window with no extra click.
            // The window stays open; closing it returns it to the tray
            // per the existing on_window_event handler.
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::add_rule,
            commands::update_rule,
            commands::delete_rule,
            commands::save_all,
            commands::import_json,
            commands::export_json,
            commands::get_app_meta,
            commands::merge_import,
        ])
        .on_window_event(|window, event| {
            // Hide instead of close when the user clicks the X. The process
            // stays alive in the tray; quitting requires ending it from Task
            // Manager in v0.1.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
