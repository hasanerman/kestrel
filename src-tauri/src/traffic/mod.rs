mod parse;
#[cfg(windows)]
mod windows;

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use parse::{Packet, TCP, UDP};

const SAMPLE_BYTES: usize = 64;
const KEEP_FLOWS: usize = 400;
const SHOWN_FLOWS: usize = 80;
const SHOWN_APPS: usize = 24;
const FLOW_IDLE_MS: i64 = 120_000;
const OWNERS_EVERY: u64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Starting,
    Running,
    NeedsAdmin,
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FlowKey {
    protocol: u8,
    local_port: u16,
    remote: Ipv4Addr,
    remote_port: u16,
}

struct Flow {
    id: u64,
    key: FlowKey,
    pid: Option<u32>,
    label: &'static str,
    name: Option<String>,
    detail: Option<String>,
    down: u64,
    up: u64,
    last: i64,
    sample: Vec<u8>,
    sample_out: bool,
}

#[derive(Default, Clone, Copy)]
struct Totals {
    down: u64,
    up: u64,
    window_down: u64,
    window_up: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppView {
    name: String,
    path: Option<String>,
    down: u64,
    up: u64,
    total_down: u64,
    total_up: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowView {
    id: u64,
    app: String,
    protocol: &'static str,
    label: &'static str,
    remote: String,
    remote_port: u16,
    local_port: u16,
    name: Option<String>,
    detail: Option<String>,
    down: u64,
    up: u64,
    last: i64,
    sample: Vec<u8>,
    sample_out: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    status: Status,
    down: u64,
    up: u64,
    average_down: u64,
    average_up: u64,
    apps: Vec<AppView>,
    flows: Vec<FlowView>,
}

pub struct Monitor {
    status: Status,
    local: Option<Ipv4Addr>,
    flows: HashMap<FlowKey, Flow>,
    names: HashMap<Ipv4Addr, String>,
    owners: HashMap<(u8, u16), u32>,
    process_names: HashMap<u32, Option<(String, String)>>,
    started: Option<Instant>,
    all_down: u64,
    all_up: u64,
    totals: HashMap<Option<u32>, Totals>,
    next_id: u64,
    latest: Option<Snapshot>,
}

fn guess_label(protocol: u8, remote_port: u16) -> &'static str {
    match (protocol, remote_port) {
        (_, 53) => "DNS",
        (UDP, 443) => "QUIC",
        (TCP, 443) => "TLS",
        (TCP, 80) => "HTTP",
        (UDP, _) => "UDP",
        _ => "TCP",
    }
}

fn dns_kind(kind: u16) -> &'static str {
    match kind {
        1 => "A",
        28 => "AAAA",
        5 => "CNAME",
        65 => "HTTPS",
        12 => "PTR",
        _ => "query",
    }
}

impl Monitor {
    fn new() -> Self {
        Self {
            status: Status::Starting,
            local: None,
            flows: HashMap::new(),
            names: HashMap::new(),
            owners: HashMap::new(),
            process_names: HashMap::new(),
            started: None,
            all_down: 0,
            all_up: 0,
            totals: HashMap::new(),
            next_id: 1,
            latest: None,
        }
    }

    fn running(&mut self, local: Ipv4Addr) {
        self.local = Some(local);
        self.status = Status::Running;
        self.started.get_or_insert_with(Instant::now);
    }

    fn observe(&mut self, packet: &Packet, now: i64) {
        let Some(local) = self.local else {
            return;
        };
        if packet.protocol != TCP && packet.protocol != UDP {
            return;
        }
        let outbound = packet.source == local;
        if !outbound && packet.destination != local {
            return;
        }
        let key = if outbound {
            FlowKey { protocol: packet.protocol, local_port: packet.source_port, remote: packet.destination, remote_port: packet.destination_port }
        } else {
            FlowKey { protocol: packet.protocol, local_port: packet.destination_port, remote: packet.source, remote_port: packet.source_port }
        };
        let pid = self.owners.get(&(key.protocol, key.local_port)).copied();
        let bytes = packet.length as u64;

        if outbound {
            self.all_up += bytes;
        } else {
            self.all_down += bytes;
        }
        let totals = self.totals.entry(pid).or_default();
        if outbound {
            totals.up += bytes;
            totals.window_up += bytes;
        } else {
            totals.down += bytes;
            totals.window_down += bytes;
        }

        let next_id = &mut self.next_id;
        let flow = self.flows.entry(key).or_insert_with(|| {
            *next_id += 1;
            Flow {
                id: *next_id,
                key,
                pid,
                label: guess_label(key.protocol, key.remote_port),
                name: None,
                detail: None,
                down: 0,
                up: 0,
                last: now,
                sample: Vec::new(),
                sample_out: outbound,
            }
        });
        flow.last = now;
        if flow.pid.is_none() {
            flow.pid = pid;
        }
        if outbound {
            flow.up += bytes;
        } else {
            flow.down += bytes;
        }

        let payload = packet.payload;
        if !payload.is_empty() {
            if key.protocol == UDP && key.remote_port == 53 {
                if let Some(dns) = parse::dns(payload) {
                    if dns.response {
                        for address in &dns.answers {
                            self.names.insert(*address, dns.name.clone());
                        }
                        let answers: Vec<String> = dns.answers.iter().map(ToString::to_string).collect();
                        flow.detail = Some(if answers.is_empty() {
                            format!("{} {} → no IPv4 answer", dns_kind(dns.kind), dns.name)
                        } else {
                            format!("{} {} → {}", dns_kind(dns.kind), dns.name, answers.join(", "))
                        });
                    } else if flow.detail.is_none() {
                        flow.detail = Some(format!("{} {}", dns_kind(dns.kind), dns.name));
                    }
                    flow.name = Some(dns.name);
                    flow.label = "DNS";
                }
            } else if key.protocol == TCP {
                if let Some(name) = parse::server_name(payload) {
                    self.names.insert(key.remote, name.clone());
                    flow.detail = Some(format!("TLS ClientHello · SNI {name}"));
                    flow.name = Some(name);
                    flow.label = "TLS";
                } else if let Some((line, host)) = parse::http_request(payload) {
                    flow.detail = Some(line);
                    if let Some(host) = host {
                        flow.name = Some(host);
                    }
                    flow.label = "HTTP";
                } else if let Some(version) = parse::tls_version(payload) {
                    if flow.label != "TLS" {
                        flow.label = if version == "TLS" { "TLS" } else { version };
                    }
                }
            }
            if flow.sample.is_empty() {
                flow.sample = payload[..payload.len().min(SAMPLE_BYTES)].to_vec();
                flow.sample_out = outbound;
            }
        }

        if self.flows.len() > KEEP_FLOWS {
            self.flows.retain(|_, f| now - f.last < FLOW_IDLE_MS);
        }
    }

    fn app(&mut self, pid: Option<u32>) -> (String, Option<String>) {
        let Some(pid) = pid else {
            return ("other".into(), None);
        };
        match self.process_names.entry(pid).or_insert_with(|| platform_process(pid)) {
            Some((name, path)) => (name.clone(), (!path.is_empty()).then(|| path.clone())),
            None => (format!("pid {pid}"), None),
        }
    }

    fn app_name(&mut self, pid: Option<u32>) -> String {
        self.app(pid).0
    }

    fn snapshot(&mut self) -> Snapshot {
        let mut grouped: HashMap<String, Totals> = HashMap::new();
        let mut paths: HashMap<String, String> = HashMap::new();
        let (mut down, mut up) = (0, 0);
        let pids: Vec<(Option<u32>, Totals)> = self.totals.iter().map(|(p, t)| (*p, *t)).collect();
        for (pid, totals) in pids {
            let (name, path) = self.app(pid);
            if let Some(path) = path {
                paths.entry(name.clone()).or_insert(path);
            }
            let entry = grouped.entry(name).or_default();
            entry.down += totals.down;
            entry.up += totals.up;
            entry.window_down += totals.window_down;
            entry.window_up += totals.window_up;
            down += totals.window_down;
            up += totals.window_up;
        }
        self.totals.values_mut().for_each(|t| {
            t.window_down = 0;
            t.window_up = 0;
        });
        let mut apps: Vec<AppView> = grouped
            .into_iter()
            .map(|(name, t)| AppView { path: paths.get(&name).cloned(), name, down: t.window_down, up: t.window_up, total_down: t.down, total_up: t.up })
            .collect();
        apps.sort_by(|a, b| (b.down + b.up).cmp(&(a.down + a.up)).then((b.total_down + b.total_up).cmp(&(a.total_down + a.total_up))));
        apps.truncate(SHOWN_APPS);

        let mut recent: Vec<(i64, u64, FlowKey)> = self.flows.values().map(|f| (f.last, f.id, f.key)).collect();
        recent.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        recent.truncate(SHOWN_FLOWS);
        let mut flows = Vec::with_capacity(recent.len());
        for (_, _, key) in recent {
            let pid = self.flows[&key].pid;
            let app = self.app_name(pid);
            let flow = &self.flows[&key];
            flows.push(FlowView {
                id: flow.id,
                app,
                protocol: if key.protocol == TCP { "tcp" } else { "udp" },
                label: flow.label,
                remote: key.remote.to_string(),
                remote_port: key.remote_port,
                local_port: key.local_port,
                name: flow.name.clone().or_else(|| self.names.get(&key.remote).cloned()),
                detail: flow.detail.clone(),
                down: flow.down,
                up: flow.up,
                last: flow.last,
                sample: flow.sample.clone(),
                sample_out: flow.sample_out,
            });
        }
        let elapsed = self.started.map_or(0.0, |s| s.elapsed().as_secs_f64());
        let (average_down, average_up) = if elapsed >= 1.0 {
            ((self.all_down as f64 / elapsed) as u64, (self.all_up as f64 / elapsed) as u64)
        } else {
            (down, up)
        };
        let snapshot = Snapshot { status: self.status, down, up, average_down, average_up, apps, flows };
        self.latest = Some(snapshot.clone());
        snapshot
    }
}

#[cfg(windows)]
fn platform_process(pid: u32) -> Option<(String, String)> {
    windows::process(pid)
}

#[cfg(not(windows))]
fn platform_process(_pid: u32) -> Option<(String, String)> {
    None
}

#[cfg(windows)]
fn platform_owners() -> HashMap<(u8, u16), u32> {
    windows::owners()
}

#[cfg(not(windows))]
fn platform_owners() -> HashMap<(u8, u16), u32> {
    HashMap::new()
}

#[cfg(windows)]
fn platform_capture(shared: &Arc<Mutex<Monitor>>) -> Status {
    windows::capture(shared)
}

#[cfg(not(windows))]
fn platform_capture(_shared: &Arc<Mutex<Monitor>>) -> Status {
    Status::Unsupported
}

fn lock(shared: &Arc<Mutex<Monitor>>) -> MutexGuard<'_, Monitor> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn local_address() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:53").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

pub struct Traffic(Arc<Mutex<Monitor>>);

impl Traffic {
    pub fn latest(&self) -> Option<Snapshot> {
        lock(&self.0).latest.clone()
    }

    pub fn rate(&self) -> Option<u64> {
        let monitor = lock(&self.0);
        let latest = monitor.latest.as_ref()?;
        (latest.status == Status::Running).then_some(latest.down + latest.up)
    }
}

pub fn start(app: AppHandle) {
    let shared = Arc::new(Mutex::new(Monitor::new()));
    app.manage(Traffic(shared.clone()));

    let capture = shared.clone();
    std::thread::spawn(move || loop {
        let status = platform_capture(&capture);
        lock(&capture).status = status;
        match status {
            Status::NeedsAdmin => {
                crate::log::warn("traffic capture needs administrator rights");
                break;
            }
            Status::Unsupported => break,
            Status::Failed => crate::log::warn("traffic capture stopped, retrying"),
            _ => {}
        }
        std::thread::sleep(Duration::from_secs(3));
    });

    tauri::async_runtime::spawn(async move {
        let mut tick: u64 = 0;
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            if tick % OWNERS_EVERY == 0 {
                if let Ok(owners) = tokio::task::spawn_blocking(platform_owners).await {
                    let mut monitor = lock(&shared);
                    if !owners.is_empty() {
                        let live: std::collections::HashSet<u32> = owners.values().copied().collect();
                        monitor.process_names.retain(|pid, _| live.contains(pid));
                    }
                    monitor.owners = owners;
                }
            }
            tick += 1;
            let snapshot = lock(&shared).snapshot();
            let _ = app.emit("traffic", &snapshot);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn udp(source: Ipv4Addr, destination: Ipv4Addr, sp: u16, dp: u16, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0x45, 0, 0, 0, 0, 0, 0, 0, 64, UDP, 0, 0];
        packet.extend_from_slice(&source.octets());
        packet.extend_from_slice(&destination.octets());
        packet.extend_from_slice(&sp.to_be_bytes());
        packet.extend_from_slice(&dp.to_be_bytes());
        packet.extend_from_slice(&[0, 0, 0, 0]);
        packet.extend_from_slice(payload);
        let total = packet.len() as u16;
        packet[2..4].copy_from_slice(&total.to_be_bytes());
        packet
    }

    #[test]
    fn names_flows_from_dns_and_counts_per_app() {
        let local = Ipv4Addr::new(192, 168, 8, 42);
        let router = Ipv4Addr::new(192, 168, 8, 1);
        let mut monitor = Monitor::new();
        monitor.running(local);
        monitor.owners.insert((UDP, 51544), 77);
        monitor.process_names.insert(77, Some(("Discord".into(), "C:\\Discord.exe".into())));

        let mut answer = vec![0xa1, 0x2f, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0];
        answer.extend_from_slice(b"\x07discord\x03com\x00\x00\x01\x00\x01");
        answer.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 162, 159, 137, 232]);
        let inbound = udp(router, local, 53, 51544, &answer);
        monitor.observe(&parse::ipv4(&inbound).unwrap(), 1_000);
        let outbound = udp(local, Ipv4Addr::new(162, 159, 137, 232), 51544, 50000, b"voice");
        monitor.observe(&parse::ipv4(&outbound).unwrap(), 2_000);

        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.apps[0].name, "Discord");
        assert_eq!(snapshot.apps[0].path.as_deref(), Some("C:\\Discord.exe"));
        assert_eq!(snapshot.down, inbound.len() as u64);
        assert_eq!(snapshot.up, outbound.len() as u64);
        let dns = snapshot.flows.iter().find(|f| f.label == "DNS").unwrap();
        assert_eq!(dns.detail.as_deref(), Some("A discord.com → 162.159.137.232"));
        let voice = snapshot.flows.iter().find(|f| f.remote_port == 50000).unwrap();
        assert_eq!(voice.name.as_deref(), Some("discord.com"));
        assert_eq!(voice.sample, b"voice");
        assert_eq!(monitor.snapshot().down, 0);
    }
}
