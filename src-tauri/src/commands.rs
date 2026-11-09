use serde::Serialize;
use tauri::{AppHandle, State};

use crate::health::history::Minute;
use crate::health::{Archive, Latest, Snapshot};
use crate::settings::{Settings, Store};
use crate::{autostart, log, tray};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Init {
    settings: Settings,
    platform: &'static str,
    version: &'static str,
    health: Option<Snapshot>,
}

#[tauri::command]
pub fn init(app: AppHandle, store: State<Store>, latest: State<Latest>, language: String) -> Init {
    if store.get().language.is_empty() {
        store.update(|s| s.set_language(&language));
        tray::refresh(&app);
    }
    Init {
        settings: store.get(),
        platform: std::env::consts::OS,
        version: env!("CARGO_PKG_VERSION"),
        health: latest.get(),
    }
}

#[tauri::command]
pub fn health(latest: State<Latest>) -> Option<Snapshot> {
    latest.get()
}

#[tauri::command]
pub fn history(archive: State<Archive>, minutes: u32) -> Vec<Minute> {
    archive.minutes(minutes.min(60 * 24 * 31))
}

#[tauri::command]
pub fn set_language(app: AppHandle, store: State<Store>, language: String) -> Settings {
    let settings = store.update(|s| s.set_language(&language));
    tray::refresh(&app);
    settings
}

#[tauri::command]
pub fn set_theme(store: State<Store>, theme: String) -> Settings {
    store.update(|s| s.set_theme(&theme))
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, store: State<Store>, enabled: bool) -> Result<Settings, String> {
    autostart::set(enabled).inspect_err(|err| log::error(format!("autostart: {err}")))?;
    let settings = store.update(|s| s.autostart = enabled);
    tray::refresh(&app);
    Ok(settings)
}

#[tauri::command]
pub fn set_notifications(store: State<Store>, enabled: bool) -> Settings {
    store.update(|s| s.notifications = enabled)
}

#[tauri::command]
pub fn get_logs() -> Vec<String> {
    log::recent()
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}
