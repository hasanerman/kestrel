use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

const INTERVAL: Duration = Duration::from_secs(30 * 60);
const MIN_GAP: Duration = Duration::from_secs(2 * 60);

#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

#[derive(Clone, Serialize)]
pub struct Available {
    pub version: String,
    pub notes: Option<String>,
}

fn info(update: &Update) -> Available {
    Available {
        version: update.version.clone(),
        notes: update.body.clone().filter(|b| !b.trim().is_empty()),
    }
}

pub fn watch(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            check(&app).await;
            tokio::time::sleep(INTERVAL).await;
        }
    });
}

pub fn check_soon(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { check(&app).await });
}

async fn check(app: &AppHandle) {
    {
        static LAST: Mutex<Option<Instant>> = Mutex::new(None);
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|at| at.elapsed() < MIN_GAP) {
            return;
        }
        *last = Some(Instant::now());
    }
    let Ok(updater) = app.updater() else {
        return;
    };
    let Ok(Some(update)) = updater.check().await else {
        return;
    };
    let available = info(&update);
    crate::log::write(format!("update {} available", available.version));
    if let Some(pending) = app.try_state::<Pending>() {
        *pending.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(update);
    }
    let _ = app.emit("update", &available);
}

pub fn available(app: &AppHandle) -> Option<Available> {
    let pending = app.try_state::<Pending>()?;
    let guard = pending.0.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().map(info)
}

pub async fn install(app: AppHandle) -> Result<(), String> {
    let update = app
        .try_state::<Pending>()
        .and_then(|p| p.0.lock().unwrap_or_else(|e| e.into_inner()).take())
        .ok_or("no update available")?;
    crate::log::write(format!("installing update {}", update.version));
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    app.restart();
}
