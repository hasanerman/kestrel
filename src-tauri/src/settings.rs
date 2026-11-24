use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockedApp {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub autostart: bool,
    pub notifications: bool,
    pub retention_days: u32,
    pub debug: bool,
    pub sites: Vec<String>,
    pub blocked: Vec<BlockedApp>,
    pub game_targets: Vec<String>,
    pub custom_targets: Vec<String>,
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
            sites: Vec::new(),
            blocked: Vec::new(),
            game_targets: Vec::new(),
            custom_targets: Vec::new(),
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

    pub fn set_sites(&mut self, sites: &[String]) {
        let mut clean: Vec<String> = Vec::new();
        for site in sites.iter().filter_map(|s| crate::sites::normalize(s)) {
            if !clean.contains(&site) {
                clean.push(site);
            }
        }
        self.sites = clean;
    }

    pub fn set_retention(&mut self, days: u32) {
        self.retention_days = match days {
            0..=7 => 7,
            8..=30 => 30,
            _ => 90,
        };
    }

    pub fn set_game_targets(&mut self, codes: &[String]) {
        let mut clean: Vec<String> = Vec::new();
        for code in codes.iter().map(|c| c.trim().to_ascii_lowercase()) {
            let valid = (2..=6).contains(&code.len()) && code.chars().all(|c| c.is_ascii_alphanumeric());
            if valid && !clean.contains(&code) && clean.len() < MAX_TARGETS {
                clean.push(code);
            }
        }
        self.game_targets = clean;
    }

    pub fn set_custom_targets(&mut self, hosts: &[String]) {
        let mut clean: Vec<String> = Vec::new();
        for host in hosts {
            let trimmed = host.trim();
            let normalized = match trimmed.parse::<std::net::Ipv4Addr>() {
                Ok(ip) if ip.is_unspecified() || ip.is_broadcast() => None,
                Ok(ip) => Some(ip.to_string()),
                Err(_) => crate::sites::normalize(trimmed),
            };
            if let Some(host) = normalized {
                if !clean.contains(&host) && clean.len() < MAX_TARGETS {
                    clean.push(host);
                }
            }
        }
        self.custom_targets = clean;
    }

    pub fn set_theme(&mut self, theme: &str) {
        self.theme = match theme {
            "light" | "dark" => theme,
            _ => "system",
        }
        .into();
    }
}

const MAX_TARGETS: usize = 12;

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
    fn sites_are_cleaned() {
        let mut settings = Settings::default();
        settings.set_sites(&["https://Example.com/x".into(), "example.com".into(), "nope".into()]);
        assert_eq!(settings.sites, vec!["example.com"]);
    }

    #[test]
    fn retention_snaps_to_choices() {
        let mut settings = Settings::default();
        settings.set_retention(3);
        assert_eq!(settings.retention_days, 7);
        settings.set_retention(30);
        assert_eq!(settings.retention_days, 30);
        settings.set_retention(400);
        assert_eq!(settings.retention_days, 90);
    }

    #[test]
    fn targets_are_cleaned() {
        let mut settings = Settings::default();
        settings.set_game_targets(&["VIE".into(), "vie".into(), "bad code!".into(), "fra".into()]);
        assert_eq!(settings.game_targets, vec!["vie", "fra"]);
        settings.set_custom_targets(&["185.40.65.1".into(), "https://Play.Example.com/x".into(), "nope".into(), "0.0.0.0".into()]);
        assert_eq!(settings.custom_targets, vec!["185.40.65.1", "play.example.com"]);
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
