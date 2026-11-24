//! `stats.db` 的 schema 版本管理。
//!
//! 用 `PRAGMA user_version` 记当前版本。打开数据库时按顺序跑缺的迁移，
//! 每个迁移自己一个事务：中途失败只回滚这一步，不会留下半个表。
//!
//! 规则：
//! - `MIGRATIONS[i]` 把库从版本 `i + 1` 迁到 `i + 2`。
//! - 迁移只做追加（加列 / 建表 / 建索引 / 回填），不改语义、不删数据。
//! - 老库的 `user_version` 是 0：那时还没有版本概念，v1 表结构是当时 `open`
//!   里 `CREATE TABLE IF NOT EXISTS` 建的，迁移 1→2 在它上面加列。

use rusqlite::Connection;

use crate::error::Result;

/// 当前代码认识的最新 schema 版本。
pub const SCHEMA_VERSION: u32 = 2;

type Migration = fn(&Connection) -> Result<()>;

const MIGRATIONS: &[Migration] = &[migrate_v1_to_v2];

/// 把 `conn` 迁到 [`SCHEMA_VERSION`]。已是最新就什么都不做。
pub fn migrate(conn: &Connection) -> Result<()> {
    debug_assert_eq!(SCHEMA_VERSION, MIGRATIONS.len() as u32 + 1);
    let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    for (index, step) in MIGRATIONS.iter().enumerate() {
        let from = index as u32 + 1;
        if version >= from {
            continue;
        }
        // user_version 不是事务性的：必须在 COMMIT 成功之后再写版本号，
        // 否则迁移回滚了版本号却留下来了，下次打开会跳过这一步。
        conn.execute_batch("BEGIN")?;
        match step(conn) {
            Ok(()) => {
                conn.execute_batch("COMMIT")?;
                conn.pragma_update(None, "user_version", from + 1)?;
            }
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(err);
            }
        }
    }
    Ok(())
}

/// v1 → v2：给 `sessions` 加新计分列，建指标 / 技能 / 课程表。
fn migrate_v1_to_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        // score_f 存浮点分（旧 score 整数列保留）；
        // scoring_version=0 是 LegacyHits，图表只横向比较同版本的分数。
        "ALTER TABLE sessions ADD COLUMN score_f REAL;
         ALTER TABLE sessions ADD COLUMN scoring_version INTEGER NOT NULL DEFAULT 0;
         ALTER TABLE sessions ADD COLUMN damage REAL NOT NULL DEFAULT 0;
         ALTER TABLE sessions ADD COLUMN kills INTEGER NOT NULL DEFAULT 0;
         ALTER TABLE sessions ADD COLUMN headshots INTEGER NOT NULL DEFAULT 0;
         ALTER TABLE sessions ADD COLUMN difficulty REAL;
         ALTER TABLE sessions ADD COLUMN playlist_id TEXT;
         ALTER TABLE sessions ADD COLUMN category TEXT;
         ALTER TABLE sessions ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1;
         UPDATE sessions SET score_f = score WHERE score_f IS NULL;
         -- 局后指标键值表：新增指标不用改表结构。
         CREATE TABLE IF NOT EXISTS session_metrics (
             session_id TEXT NOT NULL,
             key        TEXT NOT NULL,
             value      REAL NOT NULL,
             PRIMARY KEY (session_id, key)
         );
         -- 技能画像历史（M7 写）。
         CREATE TABLE IF NOT EXISTS skill_snapshots (
             at         INTEGER NOT NULL,
             skill      TEXT NOT NULL,
             value      REAL NOT NULL,
             confidence REAL NOT NULL DEFAULT 0
         );
         -- 训练课程和连播进度（M7 写）。
         CREATE TABLE IF NOT EXISTS plans (
             id         TEXT PRIMARY KEY,
             created_at INTEGER NOT NULL,
             json       TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS plan_items (
             plan_id    TEXT NOT NULL,
             idx        INTEGER NOT NULL,
             scenario   TEXT NOT NULL,
             session_id TEXT,
             done       INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (plan_id, idx)
         );",
    )?;
    Ok(())
}
