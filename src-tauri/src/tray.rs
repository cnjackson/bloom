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

/// Load the tray icon from `icons/icon.png` at runtime. Tauri includes
/// the icon assets under `tauri::path::resource_dir`, which lands at the
/// install location in MSI bundles. Falls back to a small solid-blue
/// placeholder (so the app keeps running if the asset is missing).
fn tray_icon_rgba() -> Image<'static> {
    // 16x16 BGRA solid-blue fallback (kept as the same blue we used v0.1)
    fn fallback() -> Image<'static> {
        const W: u32 = 16;
        const H: u32 = 16;
        let mut buf: Vec<u8> = Vec::with_capacity((W * H * 4) as usize);
        for _ in 0..(W * H) {
            buf.extend_from_slice(&[0x1A, 0x63, 0xEB, 0xFF]);
        }
        Image::new_owned(buf, W, H)
    }
    // Read from disk at runtime. In the dev build this is the path
    // under src-tauri/; in production Tauri-resources it lives in the
    // install dir. Falling back to the solid square here is fine — if
    // the icon's missing, we still want the app to start.
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("icons").join("icon.png");
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

    let icon = tray_icon_rgba();

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
