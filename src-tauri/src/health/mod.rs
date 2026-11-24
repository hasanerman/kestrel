mod gateway;
pub mod games;
pub mod history;
mod ping;
pub mod spike;

use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::MissedTickBehavior;

use crate::settings::Store;
use crate::{i18n, log, notify, tray};
use history::{Cause, History, Hour, Minute, Outage};
use ping::Method;
use spike::{Detector, Event, Spike};

const TICK: Duration = Duration::from_secs(1);
const KEEP: usize = 300;
const WINDOW: usize = 60;
const DOWN_AFTER: usize = 3;
const HIGH_PING: u32 = 150;
const HIGH_LOSS: f32 = 5.0;
const GATEWAY_EVERY: u64 = 15;
const SAMPLES_PER_MINUTE: usize = 60;
const RECENT_OUTAGES: u32 = 5;
const DAY_MS: i64 = 86_400_000;
const RECENT_SPIKES: u32 = 5;
const GAME_PORT: u16 = 27015;
const CUSTOM_PORT: u16 = 443;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Condition {
    Starting,
    Ok,
    Degraded,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Router,
    Internet,
    Game,
    Custom,
}

struct Target {
    id: String,
    name: String,
    ip: Ipv4Addr,
    port: u16,
    kind: Kind,
    samples: VecDeque<Option<u32>>,
}

impl Target {
    fn new(id: impl Into<String>, name: impl Into<String>, ip: Ipv4Addr, port: u16, kind: Kind) -> Self {
        Self { id: id.into(), name: name.into(), ip, port, kind, samples: VecDeque::with_capacity(KEEP) }
    }

    fn push(&mut self, sample: Option<u32>) {
        if self.samples.len() == KEEP {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    fn view(&self) -> TargetView {
        summarize(&self.id, &self.name, self.ip, self.kind, &self.samples)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetView {
    id: String,
    name: String,
    host: String,
    kind: Kind,
    last: Option<u32>,
    jitter: Option<u32>,
    loss: f32,
    failing: bool,
    history: Vec<Option<u32>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub condition: Condition,
    pub method: Method,
    pub outage: Option<Outage>,
    pub recent: Vec<Outage>,
    pub spike: Option<Spike>,
    pub spikes: Vec<Spike>,
    pub targets: Vec<TargetView>,
}

#[derive(Default)]
pub struct Latest(Mutex<Option<Snapshot>>);

impl Latest {
    pub fn get(&self) -> Option<Snapshot> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, snapshot: Snapshot) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(snapshot);
    }
}

pub struct Archive(Option<History>);

impl Archive {
    pub fn minutes(&self, minutes: u32) -> Vec<Minute> {
        let since = now_ms() - i64::from(minutes) * 60_000;
        self.run(|h| h.minutes_since(since)).unwrap_or_default()
    }

    pub fn hours(&self, days: u32) -> Vec<Hour> {
        let now = now_ms();
        let since = now - i64::from(days) * DAY_MS;
        self.run(|h| h.hours_since(since, now)).unwrap_or_default()
    }

    fn recent(&self) -> Vec<Outage> {
        self.run(|h| h.recent_outages(RECENT_OUTAGES)).unwrap_or_default()
    }

    fn spikes(&self) -> Vec<Spike> {
        self.run(|h| h.recent_spikes(RECENT_SPIKES)).unwrap_or_default()
    }

    fn run<T>(&self, action: impl FnOnce(&History) -> rusqlite::Result<T>) -> Option<T> {
        let history = self.0.as_ref()?;
        action(history).inspect_err(|err| log::error(format!("history: {err}"))).ok()
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn internet_targets() -> Vec<Target> {
    vec![
        Target::new("cloudflare", "Cloudflare", Ipv4Addr::new(1, 1, 1, 1), 443, Kind::Internet),
        Target::new("google", "Google", Ipv4Addr::new(8, 8, 8, 8), 443, Kind::Internet),
    ]
}

fn set_router(targets: &mut Vec<Target>, gateway: Option<Ipv4Addr>) {
    targets.retain(|t| t.kind != Kind::Router);
    if let Some(ip) = gateway {
        targets.insert(0, Target::new("router", "Router", ip, 80, Kind::Router));
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Extra {
    id: String,
    name: String,
    ip: Ipv4Addr,
    port: u16,
    kind: Kind,
}

#[derive(Default)]
pub struct Extras(Mutex<(u64, Vec<Extra>)>);

impl Extras {
    fn set(&self, list: Vec<Extra>) {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if guard.1 != list {
            guard.0 += 1;
            guard.1 = list;
        }
    }

    fn get(&self) -> (u64, Vec<Extra>) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn apply_extras(targets: &mut Vec<Target>, extras: &[Extra]) {
    targets.retain(|t| {
        matches!(t.kind, Kind::Router | Kind::Internet) || extras.iter().any(|e| e.id == t.id && e.ip == t.ip)
    });
    for extra in extras {
        if !targets.iter().any(|t| t.id == extra.id) {
            targets.push(Target::new(extra.id.clone(), extra.name.clone(), extra.ip, extra.port, extra.kind));
        }
    }
}

async fn resolve(host: &str) -> Option<Ipv4Addr> {
    if let Ok(ip) = host.parse() {
        return Some(ip);
    }
    tokio::net::lookup_host((host, 0)).await.ok()?.find_map(|address| match address.ip() {
        IpAddr::V4(ip) => Some(ip),
        IpAddr::V6(_) => None,
    })
}

pub async fn refresh_extras(app: AppHandle) {
    let Some(settings) = app.try_state::<Store>().map(|s| s.get()) else {
        return;
    };
    let regions = app.try_state::<games::Catalog>().map(|c| c.list()).unwrap_or_default();
    let mut list = Vec::new();
    for code in &settings.game_targets {
        let Some(region) = regions.iter().find(|r| &r.code == code) else {
            continue;
        };
        if let Ok(ip) = region.ip.parse() {
            list.push(Extra { id: format!("game:{code}"), name: format!("CS2 {}", games::city(&region.name)), ip, port: GAME_PORT, kind: Kind::Game });
        }
    }
    for host in &settings.custom_targets {
        match resolve(host).await {
            Some(ip) => list.push(Extra { id: format!("custom:{host}"), name: host.clone(), ip, port: CUSTOM_PORT, kind: Kind::Custom }),
            None => log::warn(format!("could not resolve target {host}")),
        }
    }
    if let Some(extras) = app.try_state::<Extras>() {
        extras.set(list);
    }
}

fn own_rate(app: &AppHandle) -> Option<u64> {
    app.try_state::<crate::traffic::Traffic>().and_then(|t| t.rate())
}

fn summarize(id: &str, name: &str, ip: Ipv4Addr, kind: Kind, samples: &VecDeque<Option<u32>>) -> TargetView {
    let recent: Vec<Option<u32>> = samples.iter().rev().take(WINDOW).rev().copied().collect();
    let lost = recent.iter().filter(|s| s.is_none()).count();
    let loss = if recent.is_empty() { 0.0 } else { lost as f32 * 100.0 / recent.len() as f32 };
    let answered: Vec<u32> = recent.iter().flatten().copied().collect();
    let jitter = (answered.len() > 1).then(|| {
        let total: u32 = answered.windows(2).map(|pair| pair[0].abs_diff(pair[1])).sum();
        total / (answered.len() as u32 - 1)
    });
    let failing = samples.len() >= DOWN_AFTER && samples.iter().rev().take(DOWN_AFTER).all(Option::is_none);
    TargetView {
        id: id.into(),
        name: name.into(),
        host: ip.to_string(),
        kind,
        last: samples.back().copied().flatten(),
        jitter,
        loss,
        failing,
        history: samples.iter().copied().collect(),
    }
}

fn best_internet(targets: &[TargetView]) -> Option<&TargetView> {
    targets
        .iter()
        .filter(|t| t.kind == Kind::Internet && !t.failing && t.last.is_some())
        .min_by_key(|t| t.last.unwrap_or(u32::MAX))
}

fn classify(targets: &[TargetView]) -> Condition {
    let internet: Vec<&TargetView> = targets.iter().filter(|t| t.kind == Kind::Internet).collect();
    if internet.iter().all(|t| t.history.is_empty()) {
        return Condition::Starting;
    }
    if internet.iter().all(|t| t.failing) {
        return Condition::Down;
    }
    match best_internet(targets) {
        Some(t) if t.loss > HIGH_LOSS || t.last.unwrap_or(u32::MAX) > HIGH_PING => Condition::Degraded,
        Some(_) => Condition::Ok,
        None => Condition::Degraded,
    }
}

fn cause(targets: &[TargetView]) -> Cause {
    let router_answers = targets.iter().any(|t| t.kind == Kind::Router && !t.failing && !t.history.is_empty());
    if router_answers {
        Cause::Provider
    } else {
        Cause::Local
    }
}

fn summarize_minute(at: i64, samples: &[Option<u32>]) -> Minute {
    let answered: Vec<u32> = samples.iter().flatten().copied().collect();
    let ping = (!answered.is_empty()).then(|| answered.iter().sum::<u32>() / answered.len() as u32);
    let loss = if samples.is_empty() {
        0.0
    } else {
        (samples.len() - answered.len()) as f32 * 100.0 / samples.len() as f32
    };
    Minute { at, ping, loss }
}

fn open_archive(app: &AppHandle) -> Archive {
    let Ok(dir) = app.path().app_data_dir() else {
        return Archive(None);
    };
    match History::open(&dir.join("kestrel.db")) {
        Ok(history) => {
            let retention = app.try_state::<Store>().map_or(30, |s| s.get().retention_days.max(1));
            let _ = history.close_unfinished();
            let _ = history.prune(now_ms() - i64::from(retention) * DAY_MS);
            Archive(Some(history))
        }
        Err(err) => {
            log::error(format!("could not open history: {err}"));
            Archive(None)
        }
    }
}

struct Tracker {
    condition: Condition,
    ongoing: Option<Outage>,
    recent: Vec<Outage>,
    detector: Detector,
    spikes: Vec<Spike>,
}

pub fn start(app: AppHandle) {
    let archive = open_archive(&app);
    let recent = archive.recent();
    let spikes = archive.spikes();
    app.manage(archive);
    app.manage(Latest::default());
    app.manage(Extras::default());
    tauri::async_runtime::spawn(refresh_extras(app.clone()));
    tauri::async_runtime::spawn(async move {
        let mut targets = internet_targets();
        let mut gateway = None;
        let mut extras_version = 0;
        let mut tracker = Tracker { condition: Condition::Starting, ongoing: None, recent, detector: Detector::default(), spikes };
        let mut minute: Vec<Option<u32>> = Vec::with_capacity(SAMPLES_PER_MINUTE);
        let mut tick: u64 = 0;
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if tick % GATEWAY_EVERY == 0 {
                let found = tokio::task::spawn_blocking(gateway::find).await.ok().flatten();
                if found != gateway {
                    log::write(match found {
                        Some(_) => "router found",
                        None => "no router found",
                    });
                    set_router(&mut targets, found);
                    gateway = found;
                }
            }
            tick += 1;
            if let Some(extras) = app.try_state::<Extras>() {
                let (version, list) = extras.get();
                if version != extras_version {
                    apply_extras(&mut targets, &list);
                    extras_version = version;
                }
            }

            let handles: Vec<_> = targets
                .iter()
                .map(|t| {
                    let (ip, port) = (t.ip, t.port);
                    tauri::async_runtime::spawn(async move { ping::measure(ip, port).await })
                })
                .collect();
            for (target, handle) in targets.iter_mut().zip(handles) {
                target.push(handle.await.ok().flatten());
            }

            let views: Vec<TargetView> = targets.iter().map(Target::view).collect();
            let next = classify(&views);
            minute.push(best_internet(&views).and_then(|t| t.last));
            if next != tracker.condition {
                transition(&app, &mut tracker, next, &views);
            }
            if tracker.condition != Condition::Down {
                let router = views.iter().find(|t| t.kind == Kind::Router).and_then(|t| t.last);
                let best = best_internet(&views).and_then(|t| t.last);
                let event = tracker.detector.observe(now_ms(), best, router, || spike::source(own_rate(&app), crate::speed::capacity()));
                match event {
                    Some(Event::Started(found)) => announce_spike(&app, &found),
                    Some(Event::Ended(done)) => {
                        log::write(format!("ping spike ended, peak {} ms", done.peak));
                        if let Some(archive) = app.try_state::<Archive>() {
                            archive.run(|h| h.add_spike(&done));
                            tracker.spikes = archive.spikes();
                        }
                    }
                    None => {}
                }
            }
            if minute.len() >= SAMPLES_PER_MINUTE {
                let summary = summarize_minute(now_ms(), &minute);
                if let Some(archive) = app.try_state::<Archive>() {
                    archive.run(|h| h.add_minute(&summary));
                }
                minute.clear();
            }

            let snapshot = Snapshot {
                condition: tracker.condition,
                method: ping::method(),
                outage: tracker.ongoing.clone(),
                recent: tracker.recent.clone(),
                spike: tracker.detector.ongoing.clone(),
                spikes: tracker.spikes.clone(),
                targets: views,
            };
            if let Some(latest) = app.try_state::<Latest>() {
                latest.set(snapshot.clone());
            }
            let _ = app.emit("health", &snapshot);
        }
    });
}

fn announce_spike(app: &AppHandle, found: &Spike) {
    log::warn(format!("ping spike, source {}", found.source.as_str()));
    let Some(settings) = app.try_state::<Store>().map(|s| s.get()) else {
        return;
    };
    if settings.notifications && found.source == spike::Source::Other {
        notify::show(app, i18n::labels(&settings.language).busy_other);
    }
}

fn transition(app: &AppHandle, tracker: &mut Tracker, next: Condition, views: &[TargetView]) {
    let previous = tracker.condition;
    tracker.condition = next;
    let line = format!("condition {previous:?} -> {next:?}");
    let archive = app.try_state::<Archive>();

    if next == Condition::Down {
        log::warn(line);
        let outage = Outage { start: now_ms(), end: None, cause: cause(views) };
        if let Some(archive) = &archive {
            archive.run(|h| h.begin_outage(outage.start, outage.cause));
        }
        tracker.ongoing = Some(outage);
    } else {
        log::write(line);
        if let Some(outage) = tracker.ongoing.take() {
            if let Some(archive) = &archive {
                archive.run(|h| h.end_outage(outage.start, now_ms()));
                tracker.recent = archive.recent();
            }
        }
    }

    tray::set_condition(app, next);
    let Some(store) = app.try_state::<Store>() else {
        return;
    };
    let settings = store.get();
    if !settings.notifications {
        return;
    }
    let labels = i18n::labels(&settings.language);
    match (previous, next) {
        (Condition::Ok | Condition::Degraded, Condition::Down) => {
            let local = tracker.ongoing.as_ref().is_some_and(|o| o.cause == Cause::Local);
            notify::show(app, if local { labels.lost_local } else { labels.lost });
        }
        (Condition::Down, Condition::Ok | Condition::Degraded) => notify::show(app, labels.restored),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(kind: Kind, samples: &[Option<u32>]) -> TargetView {
        summarize("t", "T", Ipv4Addr::LOCALHOST, kind, &samples.iter().copied().collect())
    }

    fn internet(samples: &[Option<u32>]) -> TargetView {
        view(Kind::Internet, samples)
    }

    #[test]
    fn computes_loss_and_jitter() {
        let v = internet(&[Some(10), Some(20), None, Some(10)]);
        assert_eq!(v.last, Some(10));
        assert_eq!(v.loss, 25.0);
        assert_eq!(v.jitter, Some(10));
        assert!(!v.failing);
    }

    #[test]
    fn classifies_conditions() {
        assert_eq!(classify(&[internet(&[])]), Condition::Starting);
        assert_eq!(classify(&[internet(&[Some(18), Some(19)])]), Condition::Ok);
        assert_eq!(classify(&[internet(&[Some(300)])]), Condition::Degraded);
        assert_eq!(classify(&[internet(&[Some(18), None, None, None])]), Condition::Down);
        assert_eq!(classify(&[internet(&[None, None, None]), internet(&[Some(20)])]), Condition::Ok);
    }

    #[test]
    fn router_does_not_count_as_internet() {
        let router = view(Kind::Router, &[Some(2), Some(2), Some(2)]);
        let down = internet(&[None, None, None]);
        assert_eq!(classify(&[router.clone(), down.clone()]), Condition::Down);
        assert_eq!(cause(&[router, down.clone()]), Cause::Provider);
        assert_eq!(cause(&[view(Kind::Router, &[None, None, None]), down.clone()]), Cause::Local);
        assert_eq!(cause(&[down]), Cause::Local);
    }

    #[test]
    fn summarizes_a_minute() {
        let minute = summarize_minute(60_000, &[Some(10), Some(20), None, None]);
        assert_eq!(minute.ping, Some(15));
        assert_eq!(minute.loss, 50.0);
        assert_eq!(summarize_minute(0, &[None]).ping, None);
    }

    #[test]
    fn extra_targets_are_added_and_removed() {
        let mut targets = internet_targets();
        let game = Extra { id: "game:vie".into(), name: "CS2 Vienna".into(), ip: Ipv4Addr::new(146, 66, 155, 66), port: GAME_PORT, kind: Kind::Game };
        apply_extras(&mut targets, std::slice::from_ref(&game));
        assert_eq!(targets.len(), 3);
        targets[2].push(Some(64));
        apply_extras(&mut targets, std::slice::from_ref(&game));
        assert_eq!(targets[2].samples.len(), 1);
        apply_extras(&mut targets, &[]);
        assert_eq!(targets.len(), 2);
    }

    #[test]
    fn router_target_is_replaced() {
        let mut targets = internet_targets();
        set_router(&mut targets, Some(Ipv4Addr::new(192, 168, 1, 1)));
        set_router(&mut targets, Some(Ipv4Addr::new(10, 0, 0, 1)));
        assert_eq!(targets.len(), 3);
        assert_eq!(targets[0].ip, Ipv4Addr::new(10, 0, 0, 1));
        set_router(&mut targets, None);
        assert_eq!(targets.len(), 2);
    }
}
