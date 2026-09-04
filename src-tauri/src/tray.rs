//! Tauri tray wiring.
//!
//! Behaviour (per `spec.md` § Tray behaviour):
//! - Single-click on the tray icon opens or focuses the rules window.
//! - No right-click menu in v0.1 (deferred to v2).
//! - Closing the rules window sends it back to the tray; the process stays alive.

use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, Manager,
};

/// Load the tray icon. Resolution order:
/// 1. The bundled resource at `icons/icon-32.png` — Tauri copies this next
///    to the binary in MSI bundles (set as `resources` in tauri.conf.json).
/// 2. The source-tree icon at `CARGO_MANIFEST_DIR/icons/icon-32.png` — useful
///    for `cargo tauri dev` runs.
/// 3. A small solid-blue 16x16 fallback so the app keeps running if no asset
///    is found at all (e.g. corrupt install).
fn tray_icon_rgba(app: &tauri::AppHandle) -> Image<'static> {
    fn fallback() -> Image<'static> {
        const W: u32 = 16;
        const H: u32 = 16;
        let mut buf: Vec<u8> = Vec::with_capacity((W * H * 4) as usize);
        for _ in 0..(W * H) {
            buf.extend_from_slice(&[0x1A, 0x63, 0xEB, 0xFF]);
        }
        Image::new_owned(buf, W, H)
    }

    // 1. Production: resource_dir is the install root; MSI copies the
    //    resources there. The PNG is embedded as a separate 256x256 / 32x32
    //    asset that Tauri can resolve via path().
    if let Ok(dir) = app.path().resource_dir() {
        let candidate = dir.join("icons").join("icon-32.png");
        if let Ok(bytes) = std::fs::read(&candidate) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let rgba = img.to_rgba8();
                let (w, h) = (rgba.width(), rgba.height());
                return Image::new_owned(rgba.into_raw(), w, h);
            }
        }
    }

    // 2. Dev: read from the source tree directly.
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("icons")
        .join("icon-32.png");
    if let Ok(bytes) = std::fs::read(&here) {
        if let Ok(img) = image::load_from_memory(&bytes) {
            let rgba = img.to_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            return Image::new_owned(rgba.into_raw(), w, h);
        }
    }

    fallback()
}

/// Install a tray icon with a single-click → open window behaviour.
/// Caller wires this in tauri::Builder::setup().
pub fn install(app: &mut App) -> tauri::Result<()> {
    let quit = MenuItem::with_id(app, "quit", "Quit Bloom", true, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Open rules", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let icon = tray_icon_rgba(app.handle());

    let _tray = TrayIconBuilder::with_id("bloom-tray")
        .tooltip("Bloom — text replacement")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => {
                app.exit(0);
            }
            _ => {
                show_or_focus_main(app);
            }
        })
        .on_tray_icon_event(|tray, event| {
            // Single-click → open/focus the rules window. Right-click is
            // handled automatically by the menu we attached above.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_or_focus_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn show_or_focus_main<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(win) = app.get_webview_window("main") {
        if win.is_visible().unwrap_or(false) {
            let _ = win.set_focus();
        } else {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}
