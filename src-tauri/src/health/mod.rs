mod gateway;
pub mod history;
mod ping;

use std::collections::VecDeque;
use std::net::Ipv4Addr;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::MissedTickBehavior;

use crate::settings::Store;
use crate::{i18n, log, notify, tray};
use history::{Cause, History, Minute, Outage};
use ping::Method;

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
}

struct Target {
    id: &'static str,
    name: &'static str,
    ip: Ipv4Addr,
    port: u16,
    kind: Kind,
    samples: VecDeque<Option<u32>>,
}

impl Target {
    fn new(id: &'static str, name: &'static str, ip: Ipv4Addr, port: u16, kind: Kind) -> Self {
        Self { id, name, ip, port, kind, samples: VecDeque::with_capacity(KEEP) }
    }

    fn push(&mut self, sample: Option<u32>) {
        if self.samples.len() == KEEP {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    fn view(&self) -> TargetView {
        summarize(self.id, self.name, self.ip, self.kind, &self.samples)
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

    fn recent(&self) -> Vec<Outage> {
        self.run(|h| h.recent_outages(RECENT_OUTAGES)).unwrap_or_default()
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
}

pub fn start(app: AppHandle) {
    let archive = open_archive(&app);
    let recent = archive.recent();
    app.manage(archive);
    app.manage(Latest::default());
    tauri::async_runtime::spawn(async move {
        let mut targets = internet_targets();
        let mut gateway = None;
        let mut tracker = Tracker { condition: Condition::Starting, ongoing: None, recent };
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
                targets: views,
            };
            if let Some(latest) = app.try_state::<Latest>() {
                latest.set(snapshot.clone());
            }
            let _ = app.emit("health", &snapshot);
        }
    });
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
