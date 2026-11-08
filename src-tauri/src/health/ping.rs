use std::time::{Duration, Instant};

use tokio::net::TcpStream;

const TIMEOUT: Duration = Duration::from_millis(1500);

pub async fn tcp(host: &str, port: u16) -> Option<u32> {
    let start = Instant::now();
    match tokio::time::timeout(TIMEOUT, TcpStream::connect((host, port))).await {
        Ok(Ok(_)) => Some((start.elapsed().as_millis() as u32).max(1)),
        _ => None,
    }
}
