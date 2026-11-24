use std::sync::atomic::{AtomicU64, Ordering};

static LAST_DOWNLOAD: AtomicU64 = AtomicU64::new(0);

pub fn capacity() -> Option<u64> {
    Some(LAST_DOWNLOAD.load(Ordering::Relaxed)).filter(|bytes| *bytes > 0)
}
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const DOWNLOAD_URL: &str = "https://speed.cloudflare.com/__down?bytes=25000000";
const UPLOAD_URL: &str = "https://speed.cloudflare.com/__up";
const STREAMS: usize = 4;
const UPLOAD_BYTES: usize = 20_000_000;
const PHASE_LIMIT: Duration = Duration::from_secs(10);
const REPORT_EVERY: Duration = Duration::from_millis(250);

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

async fn stream(client: reqwest::Client, received: Arc<AtomicU64>, start: Instant) -> std::result::Result<(), String> {
    while start.elapsed() < PHASE_LIMIT {
        let mut response = client.get(DOWNLOAD_URL).send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }
        loop {
            if start.elapsed() >= PHASE_LIMIT {
                return Ok(());
            }
            match tokio::time::timeout(Duration::from_secs(5), response.chunk()).await {
                Ok(Ok(Some(chunk))) => {
                    received.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                }
                Ok(Ok(None)) => break,
                Ok(Err(e)) => return Err(e.to_string()),
                Err(_) => return Err("timed out".into()),
            }
        }
    }
    Ok(())
}

pub async fn download(client: &reqwest::Client, report: impl Fn(f64)) -> std::result::Result<f64, String> {
    let received = Arc::new(AtomicU64::new(0));
    let start = Instant::now();
    let tasks: Vec<_> = (0..STREAMS)
        .map(|_| tokio::spawn(stream(client.clone(), received.clone(), start)))
        .collect();
    while start.elapsed() < PHASE_LIMIT && !tasks.iter().all(|t| t.is_finished()) {
        tokio::time::sleep(REPORT_EVERY).await;
        report(mbps(received.load(Ordering::Relaxed), start.elapsed()));
    }
    let elapsed = start.elapsed();
    let mut failure = None;
    for task in tasks {
        task.abort();
        if let Ok(Err(error)) = task.await {
            failure.get_or_insert(error);
        }
    }
    let total = received.load(Ordering::Relaxed);
    if total == 0 {
        return Err(failure.unwrap_or_else(|| "no data".into()));
    }
    Ok(mbps(total, elapsed))
}

async fn upload(client: &reqwest::Client) -> std::result::Result<f64, String> {
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
    let download = download(&client, |mbps| {
        let _ = app.emit("speed", Progress { phase: Phase::Download, mbps });
    })
    .await
    .inspect_err(|e| crate::log::error(format!("speed test download: {e}")))?;
    LAST_DOWNLOAD.store((download * 125_000.0) as u64, Ordering::Relaxed);
    let _ = app.emit("speed", Progress { phase: Phase::Upload, mbps: 0.0 });
    let upload = upload(&client).await.inspect_err(|e| crate::log::warn(format!("speed test upload: {e}"))).unwrap_or(0.0);
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

    #[tokio::test]
    #[ignore]
    async fn measures_real_download() {
        let client = client().unwrap();
        let result = download(&client, |mbps| println!("{mbps} Mbps")).await.unwrap();
        println!("download {result} Mbps");
        assert!(result > 0.0);
    }
}
