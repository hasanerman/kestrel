use std::collections::VecDeque;

use serde::Serialize;

const BASELINE_SAMPLES: usize = 300;
const BASELINE_MIN_SAMPLES: usize = 30;
const START_AFTER: u32 = 5;
const END_AFTER: u32 = 10;
const ROUTER_OK_MS: u32 = 30;
const OWN_FLOOR: u64 = 64_000;
const OWN_SHARE: f64 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Other,
    Own,
    Unknown,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Other => "other",
            Source::Own => "own",
            Source::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "other" => Source::Other,
            "own" => Source::Own,
            _ => Source::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Spike {
    pub start: i64,
    pub end: Option<i64>,
    pub peak: u32,
    pub source: Source,
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Started(Spike),
    Ended(Spike),
}

pub fn is_high(best: u32, baseline: u32, router: Option<u32>) -> bool {
    let limit = (baseline * 3).max(baseline + 80);
    best > limit && router.is_none_or(|r| r <= ROUTER_OK_MS)
}

pub fn source(own_rate: Option<u64>, capacity: Option<u64>) -> Source {
    let Some(rate) = own_rate else {
        return Source::Unknown;
    };
    let limit = capacity.map_or(OWN_FLOOR, |c| ((c as f64 * OWN_SHARE) as u64).max(OWN_FLOOR));
    if rate < limit {
        Source::Other
    } else {
        Source::Own
    }
}

#[derive(Default)]
pub struct Detector {
    window: VecDeque<u32>,
    high: u32,
    low: u32,
    pub ongoing: Option<Spike>,
}

impl Detector {
    pub fn baseline(&self) -> Option<u32> {
        if self.window.len() < BASELINE_MIN_SAMPLES {
            return None;
        }
        let mut sorted: Vec<u32> = self.window.iter().copied().collect();
        sorted.sort_unstable();
        Some(sorted[sorted.len() / 5].max(1))
    }

    pub fn observe(&mut self, now: i64, best: Option<u32>, router: Option<u32>, source: impl FnOnce() -> Source) -> Option<Event> {
        let Some(best) = best else {
            self.high = 0;
            return None;
        };
        let high = self.baseline().is_some_and(|baseline| is_high(best, baseline, router));
        if high {
            self.high += 1;
            self.low = 0;
        } else {
            self.low += 1;
            self.high = 0;
            if self.ongoing.is_none() {
                if self.window.len() == BASELINE_SAMPLES {
                    self.window.pop_front();
                }
                self.window.push_back(best);
            }
        }

        if let Some(spike) = &mut self.ongoing {
            spike.peak = spike.peak.max(best);
            if self.low >= END_AFTER {
                let mut done = self.ongoing.take()?;
                done.end = Some(now);
                return Some(Event::Ended(done));
            }
            return None;
        }
        if self.high >= START_AFTER {
            let spike = Spike { start: now - i64::from(START_AFTER - 1) * 1000, end: None, peak: best, source: source() };
            self.ongoing = Some(spike.clone());
            return Some(Event::Started(spike));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(detector: &mut Detector, start: i64, count: usize, ping: u32, router: Option<u32>) -> Vec<Event> {
        (0..count)
            .filter_map(|i| detector.observe(start + i as i64 * 1000, Some(ping), router, || Source::Other))
            .collect()
    }

    #[test]
    fn detects_a_spike_and_its_end() {
        let mut detector = Detector::default();
        assert!(feed(&mut detector, 0, 60, 20, Some(1)).is_empty());
        let started = feed(&mut detector, 60_000, 6, 400, Some(1));
        assert_eq!(started.len(), 1);
        assert!(matches!(&started[0], Event::Started(s) if s.source == Source::Other && s.end.is_none()));
        let ended = feed(&mut detector, 66_000, 10, 20, Some(1));
        assert!(matches!(&ended[..], [Event::Ended(s)] if s.peak == 400 && s.end == Some(75_000)));
    }

    #[test]
    fn ignores_spikes_when_the_router_is_slow_too() {
        let mut detector = Detector::default();
        feed(&mut detector, 0, 60, 20, Some(1));
        assert!(feed(&mut detector, 60_000, 10, 400, Some(200)).is_empty());
    }

    #[test]
    fn needs_a_clear_jump_over_the_baseline() {
        assert!(!is_high(70, 20, Some(1)));
        assert!(is_high(120, 20, Some(1)));
        assert!(!is_high(250, 100, None));
        assert!(is_high(310, 100, None));
    }

    #[test]
    fn blames_other_devices_only_when_this_computer_is_quiet() {
        assert_eq!(source(Some(10_000), None), Source::Other);
        assert_eq!(source(Some(200_000), None), Source::Own);
        assert_eq!(source(Some(200_000), Some(12_500_000)), Source::Other);
        assert_eq!(source(None, None), Source::Unknown);
    }
}
