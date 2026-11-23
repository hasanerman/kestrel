use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Cause {
    Provider,
    Local,
}

impl Cause {
    fn as_str(self) -> &'static str {
        match self {
            Cause::Provider => "provider",
            Cause::Local => "local",
        }
    }

    fn parse(value: &str) -> Self {
        if value == "local" {
            Cause::Local
        } else {
            Cause::Provider
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Outage {
    pub start: i64,
    pub end: Option<i64>,
    pub cause: Cause,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Minute {
    pub at: i64,
    pub ping: Option<u32>,
    pub loss: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hour {
    pub at: i64,
    pub ping: Option<u32>,
    pub loss: f32,
    pub down: u32,
    pub samples: u32,
}

const HOUR_MS: i64 = 3_600_000;

fn merge_hours(minutes: &[(i64, Option<f64>, f64, u32)], outages: &[(i64, i64)], since: i64) -> Vec<Hour> {
    let mut hours: std::collections::BTreeMap<i64, Hour> = minutes
        .iter()
        .map(|&(at, ping, loss, samples)| {
            let hour = Hour { at, ping: ping.map(|p| p.round() as u32), loss: loss as f32, down: 0, samples };
            (at, hour)
        })
        .collect();
    for &(start, end) in outages {
        let mut cursor = start.max(since);
        while cursor < end {
            let bucket = cursor - cursor.rem_euclid(HOUR_MS);
            let slice_end = end.min(bucket + HOUR_MS);
            let entry = hours
                .entry(bucket)
                .or_insert(Hour { at: bucket, ping: None, loss: 0.0, down: 0, samples: 0 });
            entry.down += ((slice_end - cursor) / 1000) as u32;
            cursor = slice_end;
        }
    }
    hours.into_values().collect()
}

pub struct History(Mutex<Connection>);

impl History {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::prepare(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(connection: Connection) -> rusqlite::Result<Self> {
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS outages (start INTEGER PRIMARY KEY, end INTEGER, cause TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS minutes (at INTEGER PRIMARY KEY, ping INTEGER, loss REAL NOT NULL);",
        )?;
        Ok(Self(Mutex::new(connection)))
    }

    fn with<T>(&self, run: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
        run(&self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn close_unfinished(&self) -> rusqlite::Result<()> {
        self.with(|db| {
            db.execute(
                "UPDATE outages SET end = MAX(start, COALESCE((SELECT MAX(at) FROM minutes WHERE at >= outages.start), start))
                 WHERE end IS NULL",
                [],
            )
            .map(|_| ())
        })
    }

    pub fn begin_outage(&self, start: i64, cause: Cause) -> rusqlite::Result<()> {
        self.with(|db| {
            db.execute("INSERT OR REPLACE INTO outages (start, end, cause) VALUES (?1, NULL, ?2)", params![start, cause.as_str()])
                .map(|_| ())
        })
    }

    pub fn end_outage(&self, start: i64, end: i64) -> rusqlite::Result<()> {
        self.with(|db| db.execute("UPDATE outages SET end = ?2 WHERE start = ?1", params![start, end]).map(|_| ()))
    }

    pub fn recent_outages(&self, limit: u32) -> rusqlite::Result<Vec<Outage>> {
        self.with(|db| {
            let mut query = db.prepare("SELECT start, end, cause FROM outages ORDER BY start DESC LIMIT ?1")?;
            let rows = query.query_map([limit], |row| {
                Ok(Outage { start: row.get(0)?, end: row.get(1)?, cause: Cause::parse(&row.get::<_, String>(2)?) })
            })?;
            rows.collect()
        })
    }

    pub fn add_minute(&self, minute: &Minute) -> rusqlite::Result<()> {
        self.with(|db| {
            db.execute(
                "INSERT OR REPLACE INTO minutes (at, ping, loss) VALUES (?1, ?2, ?3)",
                params![minute.at, minute.ping, minute.loss],
            )
            .map(|_| ())
        })
    }

    pub fn minutes_since(&self, since: i64) -> rusqlite::Result<Vec<Minute>> {
        self.with(|db| {
            let mut query = db.prepare("SELECT at, ping, loss FROM minutes WHERE at >= ?1 ORDER BY at")?;
            let rows = query.query_map([since], |row| Ok(Minute { at: row.get(0)?, ping: row.get(1)?, loss: row.get(2)? }))?;
            rows.collect()
        })
    }

    pub fn hours_since(&self, since: i64, now: i64) -> rusqlite::Result<Vec<Hour>> {
        let (minutes, outages) = self.with(|db| {
            let mut query = db.prepare(
                "SELECT (at / 3600000) * 3600000 AS hour, AVG(ping), AVG(loss), COUNT(*)
                 FROM minutes WHERE at >= ?1 GROUP BY hour ORDER BY hour",
            )?;
            let minutes: Vec<(i64, Option<f64>, f64, u32)> = query
                .query_map([since], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let mut query = db.prepare("SELECT start, COALESCE(end, ?2) FROM outages WHERE COALESCE(end, ?2) >= ?1")?;
            let outages: Vec<(i64, i64)> =
                query.query_map([since, now], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
            Ok((minutes, outages))
        })?;
        Ok(merge_hours(&minutes, &outages, since))
    }

    pub fn prune(&self, before: i64) -> rusqlite::Result<()> {
        self.with(|db| {
            db.execute("DELETE FROM minutes WHERE at < ?1", [before])?;
            db.execute("DELETE FROM outages WHERE start < ?1", [before]).map(|_| ())
        })
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_minutes_and_outages_into_hours() {
        let history = History::in_memory().unwrap();
        history.add_minute(&Minute { at: HOUR_MS + 60_000, ping: Some(10), loss: 0.0 }).unwrap();
        history.add_minute(&Minute { at: HOUR_MS + 120_000, ping: Some(20), loss: 50.0 }).unwrap();
        history.add_minute(&Minute { at: HOUR_MS + 180_000, ping: None, loss: 100.0 }).unwrap();
        history.begin_outage(2 * HOUR_MS - 60_000, Cause::Provider).unwrap();
        history.end_outage(2 * HOUR_MS - 60_000, 2 * HOUR_MS + 120_000).unwrap();

        let hours = history.hours_since(0, 3 * HOUR_MS).unwrap();
        assert_eq!(hours.len(), 2);
        assert_eq!(hours[0], Hour { at: HOUR_MS, ping: Some(15), loss: 50.0, down: 60, samples: 3 });
        assert_eq!(hours[1], Hour { at: 2 * HOUR_MS, ping: None, loss: 0.0, down: 120, samples: 0 });
    }

    #[test]
    fn records_outages_and_minutes() {
        let history = History::in_memory().unwrap();
        history.begin_outage(1_000, Cause::Provider).unwrap();
        history.end_outage(1_000, 181_000).unwrap();
        history.begin_outage(500_000, Cause::Local).unwrap();
        history.add_minute(&Minute { at: 560_000, ping: Some(18), loss: 0.0 }).unwrap();
        history.add_minute(&Minute { at: 620_000, ping: None, loss: 100.0 }).unwrap();
        history.close_unfinished().unwrap();

        let outages = history.recent_outages(5).unwrap();
        assert_eq!(outages[0], Outage { start: 500_000, end: Some(620_000), cause: Cause::Local });
        assert_eq!(outages[1], Outage { start: 1_000, end: Some(181_000), cause: Cause::Provider });
        assert_eq!(history.minutes_since(600_000).unwrap().len(), 1);

        history.prune(600_000).unwrap();
        assert_eq!(history.minutes_since(0).unwrap().len(), 1);
        assert!(history.recent_outages(5).unwrap().is_empty());
    }
}
