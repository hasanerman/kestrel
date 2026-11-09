#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod commands;
mod health;
mod i18n;
mod log;
mod notify;
mod settings;
mod sites;
mod system;
mod tray;
mod updater;

use tauri::{Manager, RunEvent, WindowEvent};

fn main() {
    let start_in_tray = std::env::args().any(|arg| arg == autostart::TRAY_ARG);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_window(app)))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::Pending::default())
        .setup(move |app| {
            if let Ok(dir) = app.path().app_log_dir() {
                log::init(dir, app.handle());
            }
            log::write(format!("kestrel {} started on {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS));
            let store = settings::Store::open(app.handle())?;
            tray::create(app.handle(), &store.get())?;
            app.manage(store);
            health::start(app.handle().clone());
            updater::watch(app.handle().clone());
            if !start_in_tray {
                tray::show_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::init,
            commands::health,
            commands::history,
            commands::set_language,
            commands::set_theme,
            commands::set_autostart,
            commands::set_notifications,
            commands::get_logs,
            commands::site_lists,
            commands::check_sites,
            commands::set_sites,
            commands::quit,
            commands::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("failed to start kestrel");

    app.run(|app, event| {
        if let RunEvent::ExitRequested { api, code, .. } = &event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen { .. } = event {
            tray::show_window(app);
        }
        let _ = app;
    });
}
