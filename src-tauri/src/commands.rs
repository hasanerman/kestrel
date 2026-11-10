use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::health::history::Minute;
use crate::health::{Archive, Condition, Latest, Snapshot};
use crate::settings::{Settings, Store};
use crate::sites::lists::{Category, CATEGORIES};
use crate::sites::{self, SiteResult};
use crate::updater::{self, Available};
use crate::{autostart, log, system, tray};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Init {
    settings: Settings,
    platform: &'static str,
    version: &'static str,
    health: Option<Snapshot>,
    update: Option<Available>,
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
        update: updater::available(&app),
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
pub fn site_lists() -> Vec<Category> {
    CATEGORIES.to_vec()
}

#[tauri::command]
pub async fn check_sites(app: AppHandle, hosts: Vec<String>) -> Vec<SiteResult> {
    let online = app
        .try_state::<Latest>()
        .and_then(|latest| latest.get())
        .is_none_or(|snapshot| snapshot.condition != Condition::Down);
    sites::check_all(&app, hosts, online).await
}

#[tauri::command]
pub fn set_sites(store: State<Store>, sites: Vec<String>) -> Settings {
    store.update(|s| s.set_sites(&sites))
}

#[tauri::command]
pub fn set_retention(store: State<Store>, days: u32) -> Settings {
    store.update(|s| s.set_retention(days))
}

#[tauri::command]
pub fn open_repository() -> Result<(), String> {
    system::open_url(system::REPOSITORY)
}

#[tauri::command]
pub fn report_issue() -> Result<(), String> {
    system::open_url(&format!("{}/issues/new", system::REPOSITORY))
}

#[tauri::command]
pub fn diagnostics(store: State<Store>, latest: State<Latest>) -> String {
    let settings = store.get();
    let mut out = vec![
        format!("kestrel {}", env!("CARGO_PKG_VERSION")),
        format!("os: {} {}", std::env::consts::OS, std::env::consts::ARCH),
        format!("language: {} theme: {}", settings.language, settings.theme),
        format!("autostart: {} notifications: {}", settings.autostart, settings.notifications),
        format!("retention: {} days, own sites: {}", settings.retention_days, settings.sites.len()),
    ];
    if let Some(snapshot) = latest.get() {
        out.push(format!("condition: {:?} method: {:?}", snapshot.condition, snapshot.method));
        out.push(format!("outages recorded: {}", snapshot.recent.len()));
    }
    out.push(String::new());
    out.extend(log::recent());
    out.join("\n")
}

#[tauri::command]
pub fn get_logs() -> Vec<String> {
    log::recent()
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    updater::install(app).await
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}
