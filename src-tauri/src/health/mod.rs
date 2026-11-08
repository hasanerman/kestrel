mod ping;

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::MissedTickBehavior;

use crate::settings::Store;
use crate::{i18n, log, notify, tray};

const TICK: Duration = Duration::from_secs(1);
const KEEP: usize = 300;
const WINDOW: usize = 60;
const DOWN_AFTER: usize = 3;
const HIGH_PING: u32 = 150;
const HIGH_LOSS: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Condition {
    Starting,
    Ok,
    Degraded,
    Down,
}

struct Target {
    id: &'static str,
    name: &'static str,
    host: &'static str,
    port: u16,
    samples: VecDeque<Option<u32>>,
}

impl Target {
    fn new(id: &'static str, name: &'static str, host: &'static str, port: u16) -> Self {
        Self { id, name, host, port, samples: VecDeque::with_capacity(KEEP) }
    }

    fn push(&mut self, sample: Option<u32>) {
        if self.samples.len() == KEEP {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    fn view(&self) -> TargetView {
        summarize(self.id, self.name, self.host, &self.samples)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetView {
    id: String,
    name: String,
    host: String,
    last: Option<u32>,
    jitter: Option<u32>,
    loss: f32,
    failing: bool,
    history: Vec<Option<u32>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub condition: Condition,
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

fn default_targets() -> Vec<Target> {
    vec![
        Target::new("cloudflare", "Cloudflare", "1.1.1.1", 443),
        Target::new("google", "Google", "8.8.8.8", 443),
    ]
}

fn summarize(id: &str, name: &str, host: &str, samples: &VecDeque<Option<u32>>) -> TargetView {
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
        host: host.into(),
        last: samples.back().copied().flatten(),
        jitter,
        loss,
        failing,
        history: samples.iter().copied().collect(),
    }
}

fn classify(targets: &[TargetView]) -> Condition {
    if targets.iter().all(|t| t.history.is_empty()) {
        return Condition::Starting;
    }
    if targets.iter().all(|t| t.failing) {
        return Condition::Down;
    }
    let best = targets.iter().filter(|t| !t.failing).min_by_key(|t| t.last.unwrap_or(u32::MAX));
    match best {
        Some(t) if t.loss > HIGH_LOSS || t.last.unwrap_or(u32::MAX) > HIGH_PING => Condition::Degraded,
        Some(_) => Condition::Ok,
        None => Condition::Down,
    }
}

pub fn start(app: AppHandle) {
    app.manage(Latest::default());
    tauri::async_runtime::spawn(async move {
        let mut targets = default_targets();
        let mut condition = Condition::Starting;
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let handles: Vec<_> = targets
                .iter()
                .map(|t| {
                    let (host, port) = (t.host, t.port);
                    tauri::async_runtime::spawn(async move { ping::tcp(host, port).await })
                })
                .collect();
            for (target, handle) in targets.iter_mut().zip(handles) {
                target.push(handle.await.ok().flatten());
            }

            let views: Vec<TargetView> = targets.iter().map(Target::view).collect();
            let next = classify(&views);
            if next != condition {
                announce(&app, condition, next);
                condition = next;
            }
            let snapshot = Snapshot { condition, targets: views };
            if let Some(latest) = app.try_state::<Latest>() {
                latest.set(snapshot.clone());
            }
            let _ = app.emit("health", &snapshot);
        }
    });
}

fn announce(app: &AppHandle, previous: Condition, next: Condition) {
    let line = format!("condition {previous:?} -> {next:?}");
    if next == Condition::Down {
        log::warn(line);
    } else {
        log::write(line);
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
        (Condition::Ok | Condition::Degraded, Condition::Down) => notify::show(app, labels.lost),
        (Condition::Down, Condition::Ok | Condition::Degraded) => notify::show(app, labels.restored),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(samples: &[Option<u32>]) -> TargetView {
        summarize("t", "T", "h", &samples.iter().copied().collect())
    }

    #[test]
    fn computes_loss_and_jitter() {
        let v = view(&[Some(10), Some(20), None, Some(10)]);
        assert_eq!(v.last, Some(10));
        assert_eq!(v.loss, 25.0);
        assert_eq!(v.jitter, Some(10));
        assert!(!v.failing);
    }

    #[test]
    fn classifies_conditions() {
        assert_eq!(classify(&[view(&[])]), Condition::Starting);
        assert_eq!(classify(&[view(&[Some(18), Some(19)])]), Condition::Ok);
        assert_eq!(classify(&[view(&[Some(300)])]), Condition::Degraded);
        assert_eq!(classify(&[view(&[Some(18), None, None, None])]), Condition::Down);
        assert_eq!(classify(&[view(&[None, None, None]), view(&[Some(20)])]), Condition::Ok);
    }
}
