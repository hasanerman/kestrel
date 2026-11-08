use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub autostart: bool,
    pub notifications: bool,
    pub retention_days: u32,
    pub debug: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: String::new(),
            theme: "system".into(),
            autostart: false,
            notifications: true,
            retention_days: 30,
            debug: false,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, path)
    }

    pub fn set_language(&mut self, lang: &str) {
        let lang = lang.to_ascii_lowercase();
        self.language = if lang.starts_with("tr") {
            "tr"
        } else if lang.starts_with("ru") {
            "ru"
        } else {
            "en"
        }
        .into();
    }

    pub fn set_theme(&mut self, theme: &str) {
        self.theme = match theme {
            "light" | "dark" => theme,
            _ => "system",
        }
        .into();
    }
}

pub struct Store {
    path: PathBuf,
    current: Mutex<Settings>,
}

impl Store {
    pub fn open(app: &AppHandle) -> tauri::Result<Self> {
        let path = app.path().app_config_dir()?.join("settings.json");
        let mut settings = Settings::load(&path);
        settings.autostart = crate::autostart::enabled();
        Ok(Self { path, current: Mutex::new(settings) })
    }

    pub fn get(&self) -> Settings {
        self.current.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> Settings {
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        change(&mut current);
        if let Err(err) = current.save(&self.path) {
            crate::log::error(format!("could not save settings: {err}"));
        }
        current.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_is_normalized() {
        let mut settings = Settings::default();
        settings.set_language("tr-TR");
        assert_eq!(settings.language, "tr");
        settings.set_language("ru-RU");
        assert_eq!(settings.language, "ru");
        settings.set_language("de-DE");
        assert_eq!(settings.language, "en");
    }

    #[test]
    fn theme_is_normalized() {
        let mut settings = Settings::default();
        settings.set_theme("dark");
        assert_eq!(settings.theme, "dark");
        settings.set_theme("purple");
        assert_eq!(settings.theme, "system");
    }
}
