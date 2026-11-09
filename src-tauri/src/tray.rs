use std::sync::Mutex;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::health::Condition;
use crate::i18n;
use crate::settings::{Settings, Store};

const ICON_IDLE: &[u8] = include_bytes!("../icons/tray/idle.png");
const ICON_OK: &[u8] = include_bytes!("../icons/tray/ok.png");
const ICON_DEGRADED: &[u8] = include_bytes!("../icons/tray/degraded.png");
const ICON_DOWN: &[u8] = include_bytes!("../icons/tray/down.png");

pub struct Tray {
    icon: TrayIcon,
    status: MenuItem<Wry>,
    open: MenuItem<Wry>,
    autostart: CheckMenuItem<Wry>,
    quit: MenuItem<Wry>,
    condition: Mutex<Condition>,
}

pub fn create(app: &AppHandle, settings: &Settings) -> tauri::Result<()> {
    let labels = i18n::labels(&settings.language);
    let status = MenuItem::with_id(app, "status", labels.starting, false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", labels.open, true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(app, "autostart", labels.autostart, true, settings.autostart, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &autostart,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let icon = TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(ICON_IDLE)?)
        .tooltip("kestrel")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| handle_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_window(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(Tray { icon, status, open, autostart, quit, condition: Mutex::new(Condition::Starting) });
    Ok(())
}

fn handle_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => show_window(app),
        "autostart" => {
            let Some(store) = app.try_state::<Store>() else {
                return;
            };
            let enabled = !store.get().autostart;
            match crate::autostart::set(enabled) {
                Ok(()) => {
                    store.update(|s| s.autostart = enabled);
                }
                Err(err) => crate::log::error(format!("autostart: {err}")),
            }
            refresh(app);
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

pub fn set_condition(app: &AppHandle, condition: Condition) {
    if let Some(tray) = app.try_state::<Tray>() {
        *tray.condition.lock().unwrap_or_else(|e| e.into_inner()) = condition;
    }
    refresh(app);
}

pub fn refresh(app: &AppHandle) {
    let (Some(tray), Some(store)) = (app.try_state::<Tray>(), app.try_state::<Store>()) else {
        return;
    };
    let settings = store.get();
    let condition = *tray.condition.lock().unwrap_or_else(|e| e.into_inner());
    let labels = i18n::labels(&settings.language);
    let text = labels.condition(condition);
    let _ = tray.status.set_text(text);
    let _ = tray.open.set_text(labels.open);
    let _ = tray.autostart.set_text(labels.autostart);
    let _ = tray.autostart.set_checked(settings.autostart);
    let _ = tray.quit.set_text(labels.quit);
    let _ = tray.icon.set_tooltip(Some(format!("kestrel · {text}")));
    let bytes = match condition {
        Condition::Starting => ICON_IDLE,
        Condition::Ok => ICON_OK,
        Condition::Degraded => ICON_DEGRADED,
        Condition::Down => ICON_DOWN,
    };
    if let Ok(image) = Image::from_bytes(bytes) {
        let _ = tray.icon.set_icon(Some(image));
    }
}

pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    crate::updater::check_soon(app);
}
