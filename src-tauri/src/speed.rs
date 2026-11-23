use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const DOWNLOAD_URL: &str = "https://speed.cloudflare.com/__down?bytes=";
const UPLOAD_URL: &str = "https://speed.cloudflare.com/__up";
const DOWNLOAD_BYTES: u64 = 100_000_000;
const UPLOAD_BYTES: usize = 20_000_000;
const PHASE_LIMIT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Download,
    Upload,
}

#[derive(Clone, Serialize)]
pub struct Progress {
    phase: Phase,
    mbps: f64,
}

#[derive(Clone, Serialize)]
pub struct Result {
    download: f64,
    upload: f64,
}

pub fn mbps(bytes: u64, elapsed: Duration) -> f64 {
    let seconds = elapsed.as_secs_f64().max(0.001);
    (bytes as f64 * 8.0 / seconds / 1_000_000.0 * 10.0).round() / 10.0
}

fn client() -> std::result::Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(6))
        .user_agent(concat!("kestrel/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

async fn download(app: &AppHandle, client: &reqwest::Client) -> std::result::Result<f64, String> {
    let mut response = client
        .get(format!("{DOWNLOAD_URL}{DOWNLOAD_BYTES}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    let mut received: u64 = 0;
    let mut reported = Instant::now();
    while start.elapsed() < PHASE_LIMIT {
        match tokio::time::timeout(Duration::from_secs(5), response.chunk()).await {
            Ok(Ok(Some(chunk))) => received += chunk.len() as u64,
            _ => break,
        }
        if reported.elapsed() >= Duration::from_millis(250) {
            reported = Instant::now();
            let _ = app.emit("speed", Progress { phase: Phase::Download, mbps: mbps(received, start.elapsed()) });
        }
    }
    if received == 0 {
        return Err("no data".into());
    }
    Ok(mbps(received, start.elapsed()))
}

async fn upload(app: &AppHandle, client: &reqwest::Client) -> std::result::Result<f64, String> {
    let _ = app.emit("speed", Progress { phase: Phase::Upload, mbps: 0.0 });
    let body = vec![0u8; UPLOAD_BYTES];
    let start = Instant::now();
    let sent = tokio::time::timeout(PHASE_LIMIT + Duration::from_secs(5), client.post(UPLOAD_URL).body(body).send()).await;
    match sent {
        Ok(Ok(response)) if response.status().is_success() => Ok(mbps(UPLOAD_BYTES as u64, start.elapsed())),
        Ok(Ok(response)) => Err(format!("HTTP {}", response.status())),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("timed out".into()),
    }
}

pub async fn run(app: AppHandle) -> std::result::Result<Result, String> {
    let client = client()?;
    crate::log::write("speed test started");
    let download = download(&app, &client).await?;
    let upload = upload(&app, &client).await.unwrap_or(0.0);
    crate::log::write(format!("speed test: {download} Mbps down, {upload} Mbps up"));
    Ok(Result { download, upload })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_bytes_to_mbps() {
        assert_eq!(mbps(12_500_000, Duration::from_secs(1)), 100.0);
        assert_eq!(mbps(1_000_000, Duration::from_secs(2)), 4.0);
    }
}
