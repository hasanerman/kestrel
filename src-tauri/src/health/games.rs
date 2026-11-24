use std::net::Ipv4Addr;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::log;

const SDR_URL: &str = "https://api.steampowered.com/ISteamApps/GetSDRConfig/v1/?appid=730";
const REFRESH: Duration = Duration::from_secs(6 * 60 * 60);
const RETRY: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Region {
    pub code: String,
    pub name: String,
    pub ip: String,
}

#[derive(Default)]
pub struct Catalog(Mutex<Vec<Region>>);

impl Catalog {
    pub fn list(&self) -> Vec<Region> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, regions: Vec<Region>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = regions;
    }
}

pub fn city(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name).trim()
}

pub fn parse(json: &serde_json::Value) -> Vec<Region> {
    let Some(pops) = json.get("pops").and_then(|p| p.as_object()) else {
        return Vec::new();
    };
    let mut regions: Vec<Region> = pops
        .iter()
        .filter_map(|(code, pop)| {
            let ip = pop
                .get("relays")?
                .as_array()?
                .iter()
                .find_map(|relay| relay.get("ipv4")?.as_str()?.parse::<Ipv4Addr>().ok())?;
            let name = pop.get("desc")?.as_str()?.to_string();
            Some(Region { code: code.clone(), name, ip: ip.to_string() })
        })
        .collect();
    regions.sort_by(|a, b| a.name.cmp(&b.name));
    regions
}

async fn fetch() -> Result<Vec<Region>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(concat!("kestrel/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = client
        .get(SDR_URL)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(parse(&json))
}

pub fn start(app: AppHandle) {
    app.manage(Catalog::default());
    tauri::async_runtime::spawn(async move {
        loop {
            match fetch().await {
                Ok(regions) if !regions.is_empty() => {
                    log::write(format!("game server list: {} regions", regions.len()));
                    if let Some(catalog) = app.try_state::<Catalog>() {
                        catalog.set(regions);
                    }
                    super::refresh_extras(app.clone()).await;
                    tokio::time::sleep(REFRESH).await;
                }
                Ok(_) => {
                    log::warn("game server list is empty");
                    tokio::time::sleep(RETRY).await;
                }
                Err(err) => {
                    log::warn(format!("game server list: {err}"));
                    tokio::time::sleep(RETRY).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_valve_regions() {
        let json = serde_json::json!({
            "pops": {
                "vie": { "desc": "Vienna (Austria)", "relays": [{ "ipv4": "146.66.155.66", "port_range": [27015, 27060] }] },
                "fra": { "desc": "Frankfurt (Germany)", "relays": [{ "ipv4": "155.133.226.68" }] },
                "empty": { "desc": "No relays" }
            }
        });
        let regions = parse(&json);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0], Region { code: "fra".into(), name: "Frankfurt (Germany)".into(), ip: "155.133.226.68".into() });
        assert_eq!(city(&regions[1].name), "Vienna");
    }
}
