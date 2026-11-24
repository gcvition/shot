//! SQLite 成绩库 `cache/stats.db`。
//!
//! 每打完一局，[`crate::viewport`] 会 `insert` 一行。菜单的历史列表和得分趋势都从这里读。
//! 删除记录时同时删 `.shot` 文件，避免回放指向幽灵对局。
//!
//! `scenario` 列存的是场景 id（文件名），展示名要再问 [`crate::sce`]。

mod migrate;

use std::path::Path;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::paths::AppPaths;
use crate::sce;

/// 一行成绩。`id` 同时是 `.shot` 文件名。
///
/// `score` 是旧的整数分（hits）。新计分走 `score_f`（浮点）；
/// `scoring_version` 标记这局用的计分规则，图表只横向比较同版本的分数。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub scenario: String,
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
    pub score_f: f64,
    pub scoring_version: i64,
    pub damage: f64,
    pub kills: i64,
    pub headshots: i64,
    pub difficulty: Option<f64>,
    pub playlist_id: Option<String>,
    pub category: Option<String>,
    /// 这局 `.shot` 的文件格式版本（v1 JSON/postcard = 1，v2 二进制 = 2）。
    pub schema_version: i64,
}

impl SessionRecord {
    /// 命中 / 总开枪。没有开枪时是 0，不是 NaN。
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

/// 打开（或创建）成绩库。旧行里的 `six-targets` 会当场改成现在的场景 id。
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
        migrate::migrate(&conn)?;
        migrate_legacy_scenario_ids(&conn)?;
        Ok(Self { conn })
    }

    /// SELECT 列列表（row_to_record 按下标读，顺序不要乱）。
    const COLUMNS: &'static str = "id, scenario, started_at, duration_ms, score, hits, misses,
        cm_per_360, dpi, fov_deg, render_width, render_height,
        trace_path, video_path, rng_seed,
        score_f, scoring_version, damage, kills, headshots,
        difficulty, playlist_id, category, schema_version";

    pub fn insert(&self, rec: &SessionRecord) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (
                id, scenario, started_at, duration_ms, score, hits, misses,
                cm_per_360, dpi, fov_deg, render_width, render_height,
                trace_path, video_path, rng_seed,
                score_f, scoring_version, damage, kills, headshots,
                difficulty, playlist_id, category, schema_version
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,
                      ?16,?17,?18,?19,?20,?21,?22,?23,?24)",
            params![
                rec.id,
                sce::migrate_scenario_id(&rec.scenario),
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
                rec.score_f,
                rec.scoring_version,
                rec.damage,
                rec.kills,
                rec.headshots,
                rec.difficulty,
                rec.playlist_id,
                rec.category,
                rec.schema_version,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<SessionRecord>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM sessions WHERE id = ?1",
            Self::COLUMNS
        ))?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_record(row)?))
        } else {
            Ok(None)
        }
    }

    /// 时间正序（最旧在前），给得分趋势图用。
    pub fn recent(&self, scenario: &str, limit: usize) -> Result<Vec<SessionRecord>> {
        let mut rows = self.recent_newest(Some(scenario), limit)?;
        rows.reverse();
        Ok(rows)
    }

    /// 新到旧。图表要用时间正序时走 [`Self::recent`]。
    pub fn recent_newest(
        &self,
        scenario: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SessionRecord>> {
        let limit = limit as i64;
        if let Some(scenario) = scenario {
            let mut stmt = self.conn.prepare(&format!(
                "SELECT {} FROM sessions WHERE scenario = ?1
                 ORDER BY started_at DESC LIMIT ?2",
                Self::COLUMNS
            ))?;
            let mut rows = stmt.query(params![sce::migrate_scenario_id(scenario), limit])?;
            collect_records(&mut rows)
        } else {
            let mut stmt = self.conn.prepare(&format!(
                "SELECT {} FROM sessions ORDER BY started_at DESC LIMIT ?1",
                Self::COLUMNS
            ))?;
            let mut rows = stmt.query(params![limit])?;
            collect_records(&mut rows)
        }
    }

    /// 删成绩行 + 指标行（同事务）。`.shot` 文件由 [`delete_session`] 处理。
    pub fn delete(&mut self, id: &str) -> Result<Option<SessionRecord>> {
        let rec = self.get(id)?;
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        tx.execute(
            "DELETE FROM session_metrics WHERE session_id = ?1",
            params![id],
        )?;
        tx.commit()?;
        Ok(rec)
    }

    pub fn latest(&self) -> Result<Option<SessionRecord>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM sessions ORDER BY started_at DESC LIMIT 1",
            Self::COLUMNS
        ))?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_record(row)?))
        } else {
            Ok(None)
        }
    }
}

/// 删 SQLite 行 + `.shot` + 可能残留的视频。若删的是「最近一局」，指针改到下一行。
pub fn delete_session(paths: &AppPaths, id: &str) -> Result<()> {
    let mut db = StatsDb::open(paths)?;
    let rec = db.delete(id)?;
    remove_session_files(paths, id, rec.as_ref());
    match crate::launch::read_last_session(paths)? {
        Some(last) if last == id => {
            if let Some(next) = db.latest()? {
                crate::launch::write_last_session(paths, &next.id)?;
            } else {
                crate::launch::clear_last_session(paths)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn remove_session_files(paths: &AppPaths, id: &str, rec: Option<&SessionRecord>) {
    let canonical = paths.session_trace(id);
    let _ = std::fs::remove_file(&canonical);
    if let Some(rec) = rec {
        if !rec.trace_path.is_empty() {
            let extra = Path::new(&rec.trace_path);
            if extra != canonical.as_path() {
                let _ = std::fs::remove_file(extra);
            }
        }
        if let Some(video) = rec.video_path.as_ref() {
            let _ = std::fs::remove_file(video);
        }
    }
    let _ = std::fs::remove_file(paths.cache.join("video").join(format!("{id}.mp4")));
}

fn migrate_legacy_scenario_ids(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET scenario = 'Sixshot Ultimate'
         WHERE scenario IN ('six-targets', 'six', '1w6ts', 'SixTargets')",
        [],
    )?;
    conn.execute(
        "UPDATE sessions SET scenario = 'Gridshot Ultimate'
         WHERE scenario IN ('grid-shot', 'grid', 'gridshot', 'GridShot')",
        [],
    )?;
    Ok(())
}

fn collect_records(rows: &mut rusqlite::Rows<'_>) -> Result<Vec<SessionRecord>> {
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row_to_record(row)?);
    }
    Ok(out)
}

fn row_to_record(row: &rusqlite::Row<'_>) -> Result<SessionRecord> {
    let scenario_raw: String = row.get(1)?;
    let scenario = sce::migrate_scenario_id(&scenario_raw);
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
        score_f: row.get(15)?,
        scoring_version: row.get(16)?,
        damage: row.get(17)?,
        kills: row.get(18)?,
        headshots: row.get(19)?,
        difficulty: row.get(20)?,
        playlist_id: row.get(21)?,
        category: row.get(22)?,
        schema_version: row.get(23)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{SessionRecord, StatsDb, migrate};
    use crate::paths::AppPaths;

    #[test]
    fn stats_db_should_return_trend_in_time_order() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        let db = StatsDb::open(&paths).unwrap();
        for (i, score) in [10_i64, 20, 15].into_iter().enumerate() {
            db.insert(&SessionRecord {
                id: format!("s{i}"),
                scenario: "Sixshot Ultimate".into(),
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
                ..sample("s", "Sixshot Ultimate", 0, 0)
            })
            .unwrap();
        }
        let trend = db.recent("Sixshot Ultimate", 30).unwrap();
        assert_eq!(
            trend.iter().map(|r| r.score).collect::<Vec<_>>(),
            vec![10, 20, 15]
        );
        assert!((trend[0].accuracy() - 10.0 / 12.0).abs() < 1e-5);
    }

    fn sample(id: &str, scenario: &str, started_at: i64, score: i64) -> SessionRecord {
        SessionRecord {
            id: id.into(),
            scenario: scenario.into(),
            started_at,
            duration_ms: 60_000,
            score,
            hits: score,
            misses: 2,
            cm_per_360: 19.05,
            dpi: 800,
            fov_deg: 103.0,
            render_width: 1920,
            render_height: 1080,
            trace_path: format!("t-{id}"),
            video_path: None,
            rng_seed: 1,
            score_f: score as f64,
            scoring_version: 0,
            damage: 0.0,
            kills: 0,
            headshots: 0,
            difficulty: None,
            playlist_id: None,
            category: None,
            schema_version: 2,
        }
    }

    #[test]
    fn recent_newest_should_filter_by_scenario() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        let db = StatsDb::open(&paths).unwrap();
        db.insert(&sample("a", "Sixshot Ultimate", 1, 10)).unwrap();
        db.insert(&sample("b", "Gridshot Ultimate", 2, 20)).unwrap();
        db.insert(&sample("c", "six-targets", 3, 30)).unwrap();
        let all = db.recent_newest(None, 20).unwrap();
        assert_eq!(
            all.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["c", "b", "a"]
        );
        let six = db.recent_newest(Some("Sixshot Ultimate"), 20).unwrap();
        assert_eq!(
            six.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["c", "a"]
        );
        assert_eq!(six[0].scenario, "Sixshot Ultimate");
    }

    #[test]
    fn delete_session_should_remove_row_files_and_last_pointer() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let db = StatsDb::open(&paths).unwrap();
        let trace_a = paths.session_trace("a");
        let trace_b = paths.session_trace("b");
        let video = paths.cache.join("video");
        std::fs::create_dir_all(&video).unwrap();
        let video_a = video.join("a.mp4");
        std::fs::write(&trace_a, b"{}").unwrap();
        std::fs::write(&trace_b, b"{}").unwrap();
        std::fs::write(&video_a, b"mp4").unwrap();
        db.insert(&SessionRecord {
            id: "a".into(),
            scenario: "Sixshot Ultimate".into(),
            started_at: 1,
            score: 10,
            hits: 10,
            misses: 1,
            trace_path: trace_a.display().to_string(),
            video_path: Some(video_a.display().to_string()),
            ..sample("a", "Sixshot Ultimate", 1, 10)
        })
        .unwrap();
        db.insert(&sample("b", "Gridshot Ultimate", 2, 20)).unwrap();
        crate::launch::write_last_session(&paths, "a").unwrap();

        super::delete_session(&paths, "a").unwrap();

        let db = StatsDb::open(&paths).unwrap();
        assert!(db.get("a").unwrap().is_none());
        assert_eq!(db.get("b").unwrap().unwrap().id, "b");
        assert!(!trace_a.exists());
        assert!(trace_b.exists());
        assert!(!video_a.exists());
        assert_eq!(
            crate::launch::read_last_session(&paths).unwrap().as_deref(),
            Some("b")
        );
    }

    #[test]
    fn open_should_migrate_v1_schema_and_keep_rows() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        // 手工建一个 user_version=0 的旧库，里面已经有成绩行。
        {
            let conn = rusqlite::Connection::open(&paths.stats_db).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (
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
                INSERT INTO sessions VALUES (
                    'old1','six-targets',7,60000,10,10,1,19.05,800,103.0,
                    1920,1080,'t-old1',NULL,5
                );",
            )
            .unwrap();
        }
        let db = StatsDb::open(&paths).unwrap();
        let version: u32 = db
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, migrate::SCHEMA_VERSION);
        let rec = db.get("old1").unwrap().unwrap();
        // 旧数据还在：id 迁移后场景名归一、score_f 回填成整数分。
        assert_eq!(rec.scenario, "Sixshot Ultimate");
        assert_eq!(rec.score, 10);
        assert!((rec.score_f - 10.0).abs() < 1e-6);
        assert_eq!(rec.scoring_version, 0);
        assert_eq!(rec.schema_version, 1);
        // 新表存在：往里写一行指标不应报错。
        db.conn
            .execute(
                "INSERT INTO session_metrics (session_id, key, value) VALUES ('old1','acc',0.9)",
                [],
            )
            .unwrap();
    }

    #[test]
    fn delete_should_drop_session_metrics_in_same_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        let mut db = StatsDb::open(&paths).unwrap();
        db.insert(&sample("m", "Sixshot Ultimate", 1, 12)).unwrap();
        db.conn
            .execute(
                "INSERT INTO session_metrics (session_id, key, value) VALUES ('m','acc',0.8)",
                [],
            )
            .unwrap();
        db.delete("m").unwrap();
        let leftover: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM session_metrics WHERE session_id = 'm'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(leftover, 0);
    }
}
