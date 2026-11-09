mod dns;
pub mod lists;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::net::TcpStream;
use tokio::time::timeout;

const STEP_TIMEOUT: Duration = Duration::from_secs(6);
const HTTPS_TIMEOUT: Duration = Duration::from_secs(10);
const BLOCK_PAGES: &[Ipv4Addr] = &[Ipv4Addr::new(195, 175, 254, 2)];
const MAX_HOSTS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Works,
    DnsBlocked,
    DpiBlocked,
    Certificate,
    Unreachable,
    Down,
    NoInternet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Failure {
    Dns,
    BlockPage,
    Timeout,
    Reset,
    Refused,
    Closed,
    Certificate,
    Server,
    Other,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteResult {
    host: String,
    verdict: Verdict,
    dns: Option<u32>,
    dns_differs: bool,
    tcp: Option<u32>,
    https: Option<u32>,
    status: Option<u16>,
    failure: Option<Failure>,
}

impl SiteResult {
    fn new(host: &str) -> Self {
        Self {
            host: host.into(),
            verdict: Verdict::Works,
            dns: None,
            dns_differs: false,
            tcp: None,
            https: None,
            status: None,
            failure: None,
        }
    }

    fn fail(mut self, failure: Failure, verdict: Verdict) -> Self {
        self.failure = Some(failure);
        self.verdict = verdict;
        self
    }
}

pub fn is_hostname(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.contains('.')
        && host.split('.').all(|label| !label.is_empty() && label.len() <= 63)
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

pub fn normalize(input: &str) -> Option<String> {
    let lower = input.trim().to_ascii_lowercase();
    let without_scheme = lower.split_once("://").map_or(lower.as_str(), |(_, rest)| rest);
    let host = without_scheme
        .split(['/', '?', '#'])
        .next()?
        .rsplit('@')
        .next()?
        .split(':')
        .next()?
        .trim_end_matches('.');
    let host = host.strip_prefix("www.").unwrap_or(host);
    is_hostname(host).then(|| host.to_string())
}

fn bogon(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_unspecified() || v4.is_loopback() || v4.is_private() || v4.is_link_local() || BLOCK_PAGES.contains(v4)
        }
        IpAddr::V6(v6) => v6.is_unspecified() || v6.is_loopback(),
    }
}

fn classify(error: &reqwest::Error) -> Failure {
    let mut chain = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(inner) = source {
        chain.push_str(": ");
        chain.push_str(&inner.to_string());
        source = inner.source();
    }
    classify_text(&chain, error.is_timeout())
}

fn classify_text(chain: &str, timed_out: bool) -> Failure {
    let lower = chain.to_ascii_lowercase();
    if timed_out || lower.contains("timed out") {
        Failure::Timeout
    } else if lower.contains("certificate") || lower.contains("unknownissuer") || lower.contains("unknown issuer") {
        Failure::Certificate
    } else if lower.contains("reset") || lower.contains("10054") || lower.contains("forcibly closed") {
        Failure::Reset
    } else if lower.contains("refused") || lower.contains("10061") {
        Failure::Refused
    } else if lower.contains("eof") || lower.contains("handshake") || lower.contains("closed") {
        Failure::Closed
    } else {
        Failure::Other
    }
}

fn verdict_for(failure: Failure, dns_differs: bool) -> Verdict {
    match failure {
        Failure::Certificate => Verdict::Certificate,
        Failure::Timeout | Failure::Reset | Failure::Closed if dns_differs => Verdict::DnsBlocked,
        Failure::Timeout | Failure::Reset | Failure::Closed => Verdict::DpiBlocked,
        Failure::Refused | Failure::Server => Verdict::Down,
        Failure::Dns | Failure::BlockPage => Verdict::DnsBlocked,
        Failure::Other => Verdict::Unreachable,
    }
}

fn builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(STEP_TIMEOUT)
        .timeout(HTTPS_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .pool_max_idle_per_host(0)
        .user_agent(concat!("kestrel/", env!("CARGO_PKG_VERSION")))
}

async fn check(host: String, online: bool, doh: reqwest::Client) -> SiteResult {
    let result = SiteResult::new(&host);
    if !online {
        return result.fail(Failure::Other, Verdict::NoInternet);
    }

    let (system, secure) = tokio::join!(dns::system(&host), dns::secure(&doh, &host));
    let (ips, dns_time) = match system {
        Ok(found) => found,
        Err(()) => {
            let verdict = if secure.is_empty() { Verdict::Down } else { Verdict::DnsBlocked };
            return result.fail(Failure::Dns, verdict);
        }
    };
    let mut result = SiteResult { dns: Some(dns_time), ..result };
    if ips.iter().any(bogon) {
        return result.fail(Failure::BlockPage, Verdict::DnsBlocked);
    }
    result.dns_differs = !secure.is_empty() && secure.iter().all(|ip| !ips.contains(ip));
    let ip = ips[0];

    let start = Instant::now();
    match timeout(STEP_TIMEOUT, TcpStream::connect(SocketAddr::new(ip, 443))).await {
        Ok(Ok(_)) => result.tcp = Some(start.elapsed().as_millis() as u32),
        Ok(Err(err)) => {
            let failure = classify_text(&err.to_string(), false);
            let failure = if failure == Failure::Other { Failure::Reset } else { failure };
            let verdict = verdict_for(failure, result.dns_differs);
            return result.fail(failure, verdict);
        }
        Err(_) => {
            let verdict = if result.dns_differs { Verdict::DnsBlocked } else { Verdict::Unreachable };
            return result.fail(Failure::Timeout, verdict);
        }
    }

    let Ok(pinned) = builder().resolve(&host, SocketAddr::new(ip, 443)).build() else {
        return result.fail(Failure::Other, Verdict::Unreachable);
    };
    let start = Instant::now();
    match pinned.get(format!("https://{host}/")).send().await {
        Ok(response) => {
            let status = response.status();
            result.https = Some(start.elapsed().as_millis() as u32);
            result.status = Some(status.as_u16());
            if status.is_server_error() {
                return result.fail(Failure::Server, Verdict::Down);
            }
            result
        }
        Err(err) => {
            let failure = classify(&err);
            let verdict = verdict_for(failure, result.dns_differs);
            result.fail(failure, verdict)
        }
    }
}

pub async fn check_all(app: &AppHandle, hosts: Vec<String>, online: bool) -> Vec<SiteResult> {
    let doh = builder().build().unwrap_or_default();
    let mut unique: Vec<String> = Vec::new();
    for host in hosts.iter().filter_map(|h| normalize(h)) {
        if !unique.contains(&host) && unique.len() < MAX_HOSTS {
            unique.push(host);
        }
    }
    crate::log::write(format!("checking {} sites", unique.len()));
    let handles: Vec<_> = unique
        .into_iter()
        .map(|host| {
            let app = app.clone();
            let doh = doh.clone();
            tauri::async_runtime::spawn(async move {
                let result = check(host, online, doh).await;
                let _ = app.emit("site", &result);
                result
            })
        })
        .collect();
    let mut results = Vec::new();
    for handle in handles {
        if let Ok(result) = handle.await {
            results.push(result);
        }
    }
    let failing = results.iter().filter(|r| r.verdict != Verdict::Works).count();
    crate::log::write(format!("site check done, {failing} not working"));
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_user_input() {
        assert_eq!(normalize("https://www.Discord.com/app?x=1").as_deref(), Some("discord.com"));
        assert_eq!(normalize("github.com:443").as_deref(), Some("github.com"));
        assert_eq!(normalize("not a domain"), None);
        assert_eq!(normalize("localhost"), None);
        assert_eq!(normalize("a.com&type=AAAA"), None);
    }

    #[test]
    fn detects_block_page_answers() {
        assert!(bogon(&"0.0.0.0".parse().unwrap()));
        assert!(bogon(&"195.175.254.2".parse().unwrap()));
        assert!(!bogon(&"162.159.135.232".parse().unwrap()));
    }

    #[test]
    fn classifies_errors() {
        assert_eq!(classify_text("connection reset by peer", false), Failure::Reset);
        assert_eq!(classify_text("An existing connection was forcibly closed (os error 10054)", false), Failure::Reset);
        assert_eq!(classify_text("invalid peer certificate: UnknownIssuer", false), Failure::Certificate);
        assert_eq!(classify_text("anything", true), Failure::Timeout);
        assert_eq!(classify_text("unexpected eof during handshake", false), Failure::Closed);
    }

    #[tokio::test]
    #[ignore]
    async fn checks_real_sites() {
        let doh = builder().build().unwrap();
        for host in ["github.com", "discord.com", "wikipedia.org", "does-not-exist-kestrel.example"] {
            let result = check(host.into(), true, doh.clone()).await;
            println!("{result:?}");
        }
    }

    #[test]
    fn picks_verdicts() {
        assert_eq!(verdict_for(Failure::Reset, false), Verdict::DpiBlocked);
        assert_eq!(verdict_for(Failure::Reset, true), Verdict::DnsBlocked);
        assert_eq!(verdict_for(Failure::Certificate, false), Verdict::Certificate);
        assert_eq!(verdict_for(Failure::Refused, false), Verdict::Down);
    }
}
