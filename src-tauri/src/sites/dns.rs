use std::net::IpAddr;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::time::timeout;

const ENDPOINTS: &[&str] = &["https://1.1.1.1/dns-query", "https://8.8.8.8/resolve"];
const A_RECORD: u16 = 1;
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
struct Reply {
    #[serde(rename = "Answer", default)]
    answer: Vec<Answer>,
}

#[derive(Deserialize)]
struct Answer {
    #[serde(rename = "type")]
    kind: u16,
    data: String,
}

pub async fn system(host: &str) -> Result<(Vec<IpAddr>, u32), ()> {
    let start = Instant::now();
    match timeout(LOOKUP_TIMEOUT, tokio::net::lookup_host((host, 443))).await {
        Ok(Ok(addresses)) => {
            let mut ips: Vec<IpAddr> = addresses.map(|a| a.ip()).collect();
            ips.sort_by_key(|ip| ip.is_ipv6());
            ips.dedup();
            if ips.is_empty() {
                Err(())
            } else {
                Ok((ips, start.elapsed().as_millis() as u32))
            }
        }
        _ => Err(()),
    }
}

pub async fn secure(client: &reqwest::Client, host: &str) -> Vec<IpAddr> {
    if !super::is_hostname(host) {
        return Vec::new();
    }
    for endpoint in ENDPOINTS {
        let request = client
            .get(format!("{endpoint}?name={host}&type=A"))
            .header("accept", "application/dns-json")
            .timeout(LOOKUP_TIMEOUT)
            .send();
        let Ok(response) = request.await else {
            continue;
        };
        let Ok(reply) = response.json::<Reply>().await else {
            continue;
        };
        let ips: Vec<IpAddr> = reply
            .answer
            .into_iter()
            .filter(|a| a.kind == A_RECORD)
            .filter_map(|a| a.data.parse().ok())
            .collect();
        if !ips.is_empty() {
            return ips;
        }
    }
    Vec::new()
}
