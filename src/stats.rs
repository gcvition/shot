use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::paths::AppPaths;
use crate::settings::ScenarioKind;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub scenario: ScenarioKind,
    pub started_at: i64,
    pub duration_ms: u32,
    pub score: i64,
    pub hits: i64,
    pub misses: i64,
    pub cm_per_360: f64,
    pub dpi: u32,
    pub fov_deg: f32,
    pub render_width: u32,
    pub render_height: u32,
    pub trace_path: String,
    pub video_path: Option<String>,
    pub rng_seed: i64,
}

impl SessionRecord {
    pub fn accuracy(&self) -> f32 {
        let shots = self.hits + self.misses;
        if shots == 0 {
            0.0
        } else {
            self.hits as f32 / shots as f32
        }
    }

    pub fn hits_per_sec(&self) -> f32 {
        if self.duration_ms == 0 {
            0.0
        } else {
            self.hits as f32 / (self.duration_ms as f32 / 1000.0)
        }
    }
}

pub struct StatsDb {
    conn: Connection,
}

impl StatsDb {
    pub fn open(paths: &AppPaths) -> Result<Self> {
        paths.ensure_dirs()?;
        let conn = Connection::open(&paths.stats_db)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id            TEXT PRIMARY KEY,
                scenario      TEXT NOT NULL,
                started_at    INTEGER NOT NULL,
                duration_ms   INTEGER NOT NULL,
                score         INTEGER NOT NULL,
                hits          INTEGER NOT NULL,
                misses        INTEGER NOT NULL,
                cm_per_360    REAL NOT NULL,
                dpi           INTEGER NOT NULL,
                fov_deg       REAL NOT NULL,
                render_width  INTEGER NOT NULL,
                render_height INTEGER NOT NULL,
                trace_path    TEXT NOT NULL,
                video_path    TEXT,
                rng_seed      INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_scenario_time
                ON sessions(scenario, started_at);",
        )?;
        Ok(Self { conn })
    }

    pub fn insert(&self, rec: &SessionRecord) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (
                id, scenario, started_at, duration_ms, score, hits, misses,
                cm_per_360, dpi, fov_deg, render_width, render_height,
                trace_path, video_path, rng_seed
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                rec.id,
                rec.scenario.as_str(),
                rec.started_at,
                rec.duration_ms,
                rec.score,
                rec.hits,
                rec.misses,
                rec.cm_per_360,
                rec.dpi,
                rec.fov_deg,
                rec.render_width,
                rec.render_height,
                rec.trace_path,
                rec.video_path,
                rec.rng_seed,
            ],
        )?;
        Ok(())
    }

    pub fn set_video_path(&self, id: &str, video_path: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET video_path = ?1 WHERE id = ?2",
            params![video_path, id],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<SessionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scenario, started_at, duration_ms, score, hits, misses,
                    cm_per_360, dpi, fov_deg, render_width, render_height,
                    trace_path, video_path, rng_seed
             FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_record(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn recent(&self, scenario: ScenarioKind, limit: usize) -> Result<Vec<SessionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scenario, started_at, duration_ms, score, hits, misses,
                    cm_per_360, dpi, fov_deg, render_width, render_height,
                    trace_path, video_path, rng_seed
             FROM sessions WHERE scenario = ?1
             ORDER BY started_at DESC LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![scenario.as_str(), limit as i64])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_record(row)?);
        }
        out.reverse();
        Ok(out)
    }

    pub fn latest(&self) -> Result<Option<SessionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scenario, started_at, duration_ms, score, hits, misses,
                    cm_per_360, dpi, fov_deg, render_width, render_height,
                    trace_path, video_path, rng_seed
             FROM sessions ORDER BY started_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_record(row)?))
        } else {
            Ok(None)
        }
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> Result<SessionRecord> {
    let scenario_raw: String = row.get(1)?;
    let scenario = ScenarioKind::parse(&scenario_raw).unwrap_or(ScenarioKind::SixTargets);
    Ok(SessionRecord {
        id: row.get(0)?,
        scenario,
        started_at: row.get(2)?,
        duration_ms: row.get::<_, i64>(3)? as u32,
        score: row.get(4)?,
        hits: row.get(5)?,
        misses: row.get(6)?,
        cm_per_360: row.get(7)?,
        dpi: row.get::<_, i64>(8)? as u32,
        fov_deg: row.get::<_, f64>(9)? as f32,
        render_width: row.get::<_, i64>(10)? as u32,
        render_height: row.get::<_, i64>(11)? as u32,
        trace_path: row.get(12)?,
        video_path: row.get(13)?,
        rng_seed: row.get(14)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{SessionRecord, StatsDb};
    use crate::paths::AppPaths;
    use crate::settings::ScenarioKind;

    #[test]
    fn stats_db_should_return_trend_in_time_order() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        let db = StatsDb::open(&paths).unwrap();
        for (i, score) in [10_i64, 20, 15].into_iter().enumerate() {
            db.insert(&SessionRecord {
                id: format!("s{i}"),
                scenario: ScenarioKind::SixTargets,
                started_at: 1_000 + i as i64,
                duration_ms: 60_000,
                score,
                hits: score,
                misses: 2,
                cm_per_360: 19.05,
                dpi: 800,
                fov_deg: 103.0,
                render_width: 1920,
                render_height: 1080,
                trace_path: format!("t{i}"),
                video_path: None,
                rng_seed: i as i64,
            })
            .unwrap();
        }
        let trend = db.recent(ScenarioKind::SixTargets, 30).unwrap();
        assert_eq!(trend.iter().map(|r| r.score).collect::<Vec<_>>(), vec![10, 20, 15]);
        assert!((trend[0].accuracy() - 10.0 / 12.0).abs() < 1e-5);
    }
}
