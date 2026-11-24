//! M5：分析引擎。从 `SessionFile` 算出跨场景可比的指标。
//!
//! 全部是纯函数：读轨迹文件 → 出指标表。同一局任何时刻重算结果一致，
//! 老的 v1 轨迹缺字段时对应指标算成 0/NaN，由调用方显示「不可用」。

use std::collections::HashMap;

use crate::session::{HitPart, SessionFile, WorldKind};

/// 一局的核心指标。全部 f64；没有数据的项为 NaN（UI 显示「-」）。
#[derive(Clone, Debug, Default)]
pub struct SessionMetrics {
    /// 扳机次数。
    pub shots: f64,
    /// 命中次数（含多弹丸只算一次扳机命中）。
    pub hits: f64,
    pub misses: f64,
    pub kills: f64,
    pub damage: f64,
    /// 命中率 0..1。
    pub accuracy: f64,
    /// 爆头率（命中里 part=Head 的比例）。
    pub headshot_rate: f64,
    /// 平均首发反应时间（毫秒）：每个目标首次出生→首次命中。
    pub avg_reaction_ms: f64,
    /// 平均击杀用时（毫秒）：目标出生→消失。
    pub avg_ttk_ms: f64,
    /// 准星在目标上的时间占比（跟枪场景）。
    pub on_target_pct: f64,
    /// 最长连续命中（不出 miss 中断）。
    pub best_streak: f64,
    /// 移动时间占比（有 PlayerSample 才有值）。
    pub move_pct: f64,
}

impl SessionMetrics {
    /// 摊平成 (key, value) 写 `session_metrics` 表。
    pub fn rows(&self) -> Vec<(&'static str, f64)> {
        vec![
            ("shots", self.shots),
            ("hits", self.hits),
            ("misses", self.misses),
            ("kills", self.kills),
            ("damage", self.damage),
            ("accuracy", self.accuracy),
            ("headshot_rate", self.headshot_rate),
            ("avg_reaction_ms", self.avg_reaction_ms),
            ("avg_ttk_ms", self.avg_ttk_ms),
            ("on_target_pct", self.on_target_pct),
            ("best_streak", self.best_streak),
            ("move_pct", self.move_pct),
        ]
    }
}

/// 从一份轨迹算指标。v1 轨迹（没有 targets/keys/on_target）对应指标为 NaN。
pub fn analyze(file: &SessionFile) -> SessionMetrics {
    let mut m = SessionMetrics::default();
    m.shots = file.shots.len() as f64;
    m.hits = file.shots.iter().filter(|s| s.hit).count() as f64;
    m.misses = m.shots - m.hits;
    if m.shots > 0.0 {
        m.accuracy = m.hits / m.shots;
    } else {
        m.accuracy = f64::NAN; // 没有开火数据：NaN，不是 0%（避免拉低 AVG）。
    }
    // 爆头率：所有 ShotHit 里 Head 的比例。
    let total_parts: usize = file.shots.iter().map(|s| s.hits.len()).sum();
    if total_parts > 0 {
        let heads: usize = file
            .shots
            .iter()
            .flat_map(|s| s.hits.iter())
            .filter(|h| h.part == HitPart::Head as u8)
            .count();
        m.headshot_rate = heads as f64 / total_parts as f64;
    }
    m.damage = file
        .shots
        .iter()
        .flat_map(|s| s.hits.iter())
        .map(|h| f64::from(h.damage))
        .sum();
    // 击杀只算 WorldKind::Killed（寿终 Despawn / No-Go 误杀不算）。
    // 旧轨迹没有 Killed 事件 → 回退数 Despawn（近似）。
    let killed = file
        .world
        .iter()
        .filter(|w| w.kind == WorldKind::Killed)
        .count();
    m.kills = if killed > 0 {
        killed as f64
    } else {
        file.world
            .iter()
            .filter(|w| w.kind == WorldKind::Despawn)
            .count() as f64
    };

    // 反应时间 / TTK：按「出生实例」配对——事件流顺序扫，Spawn 进表，
    // 首次命中/被杀消费掉，id 复用（Gridshot 格位）也能算对。
    let mut spawn_t: HashMap<u32, u64> = HashMap::new();
    let mut kill_t: HashMap<u32, u64> = HashMap::new();
    let mut rsum = 0u64;
    let mut rn = 0u64;
    let mut shots_sorted: Vec<&crate::session::ShotEvent> = file.shots.iter().collect();
    shots_sorted.sort_by_key(|s| s.t_us);
    let mut shot_i = 0usize;
    for w in &file.world {
        match w.kind {
            WorldKind::Spawn => {
                // 先把这次 Spawn 之前的命中消费掉（配对到上一个实例）。
                while shot_i < shots_sorted.len() && shots_sorted[shot_i].t_us <= w.t_us {
                    let s = shots_sorted[shot_i];
                    if s.hit && s.target_id == w.id {
                        if let Some(st) = spawn_t.remove(&s.target_id) {
                            rsum += s.t_us - st;
                            rn += 1;
                        }
                    }
                    shot_i += 1;
                }
                spawn_t.insert(w.id, w.t_us);
            }
            WorldKind::Killed | WorldKind::Despawn => {
                while shot_i < shots_sorted.len() && shots_sorted[shot_i].t_us <= w.t_us {
                    let s = shots_sorted[shot_i];
                    if s.hit && s.target_id == w.id {
                        if let Some(st) = spawn_t.remove(&s.target_id) {
                            rsum += s.t_us - st;
                            rn += 1;
                        }
                    }
                    shot_i += 1;
                }
                if w.kind == WorldKind::Killed && !kill_t.contains_key(&w.id) {
                    kill_t.insert(w.id, w.t_us);
                }
            }
            _ => {}
        }
    }
    while shot_i < shots_sorted.len() {
        let s = shots_sorted[shot_i];
        if s.hit && s.target_id != 0 {
            if let Some(st) = spawn_t.remove(&s.target_id) {
                rsum += s.t_us - st;
                rn += 1;
            }
        }
        shot_i += 1;
    }
    m.avg_reaction_ms = if rn > 0 { rsum as f64 / rn as f64 / 1000.0 } else { f64::NAN };

    // TTK：「Spawn → Killed」。寿终消失的不算（那不是打死的速度）。
    let mut spawn_all: HashMap<u32, u64> = HashMap::new();
    let mut tsum = 0u64;
    let mut tn = 0u64;
    for w in &file.world {
        match w.kind {
            WorldKind::Spawn => {
                spawn_all.insert(w.id, w.t_us);
            }
            WorldKind::Killed => {
                if let Some(st) = spawn_all.remove(&w.id)
                    && w.t_us >= st
                {
                    tsum += w.t_us - st;
                    tn += 1;
                }
            }
            _ => {}
        }
    }
    m.avg_ttk_ms = if tn > 0 { tsum as f64 / tn as f64 / 1000.0 } else { f64::NAN };

    // 跟枪占比：OnTargetSpan 总长 / 对局时长。
    if file.header.duration_ms > 0 && !file.on_target.is_empty() {
        let on: u64 = file
            .on_target
            .iter()
            .map(|s| s.end_us.saturating_sub(s.start_us))
            .sum();
        m.on_target_pct = on as f64 / (file.header.duration_ms as f64 * 1000.0);
    }

    // 最长连中。
    let mut cur = 0i64;
    for s in &file.shots {
        if s.hit {
            cur += 1;
            m.best_streak = m.best_streak.max(cur as f64);
        } else {
            cur = 0;
        }
    }

    // 移动占比。
    if !file.player.is_empty() {
        let moving = file
            .player
            .iter()
            .filter(|s| (s.vel.x * s.vel.x + s.vel.z * s.vel.z).sqrt() > 0.5)
            .count();
        m.move_pct = moving as f64 / file.player.len() as f64;
    } else {
        m.move_pct = f64::NAN;
    }
    m
}

/// 按技能权重表把指标映射到技能分（M7 课程用，先给简单版）。
/// `weights` = [(skill, w)]，返回 (skill, score 0..100)。
pub fn skill_scores(m: &SessionMetrics, weights: &[(String, f32)]) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    for (skill, w) in weights {
        let base = match skill.as_str() {
            "flick" | "clicking" => {
                if m.avg_reaction_ms.is_nan() {
                    m.accuracy * 100.0
                } else {
                    (1000.0 - m.avg_reaction_ms.min(1000.0)) / 10.0 * 0.6
                        + m.accuracy * 100.0 * 0.4
                }
            }
            "tracking" => {
                if m.on_target_pct.is_nan() {
                    m.accuracy * 100.0
                } else {
                    m.on_target_pct * 100.0
                }
            }
            "reaction" | "reactive" => {
                if m.avg_reaction_ms.is_nan() {
                    50.0
                } else {
                    (1000.0 - m.avg_reaction_ms.min(1000.0)) / 10.0
                }
            }
            _ => m.accuracy * 100.0,
        };
        out.push((skill.clone(), base.clamp(0.0, 100.0) * f64::from(*w)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{SessionFile, ShotEvent, ShotHit, WorldEvent};
    use crate::vec3::Vec3;

    fn shot(t_us: u64, hit: bool, id: u32) -> ShotEvent {
        ShotEvent {
            t_us,
            hit,
            target_id: id,
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            spray_index: 0,
            punch_yaw_deg: 0.0,
            punch_pitch_deg: 0.0,
            spread_x: 0.0,
            spread_y: 0.0,
            player_vel: Vec3::ZERO,
            ammo_after: -1,
            hits: if hit {
                vec![ShotHit {
                    target_id: id,
                    part: HitPart::Body as u8,
                    damage: 25.0,
                    pos: Vec3::ZERO,
                }]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn metrics_should_count_accuracy_streak_and_reaction() {
        let mut f = SessionFile::new("s1".into(), "T", 7, Default::default());
        f.header.duration_ms = 10_000;
        f.world.push(WorldEvent {
            t_us: 0,
            kind: WorldKind::Spawn,
            id: 1,
            pos: Vec3::ZERO,
            radius: 0.3,
        });
        f.world.push(WorldEvent {
            t_us: 500_000,
            kind: WorldKind::Killed,
            id: 1,
            pos: Vec3::ZERO,
            radius: 0.0,
        });
        f.shots.push(shot(200_000, true, 1));
        f.shots.push(shot(300_000, false, 0));
        f.shots.push(shot(400_000, true, 1));
        f.shots.push(shot(500_000, true, 1));
        let m = analyze(&f);
        assert_eq!(m.shots, 4.0);
        assert_eq!(m.hits, 3.0);
        assert!((m.accuracy - 0.75).abs() < 1e-9);
        assert_eq!(m.kills, 1.0);
        assert_eq!(m.best_streak, 2.0);
        assert!((m.avg_reaction_ms - 200.0).abs() < 1e-6);
        assert!((m.avg_ttk_ms - 500.0).abs() < 1e-6);
    }

    #[test]
    fn v1_trace_should_give_nan_for_missing_fields() {
        let mut f = SessionFile::new("s2".into(), "Old", 1, Default::default());
        f.header.duration_ms = 1000;
        let m = analyze(&f);
        assert!(m.avg_reaction_ms.is_nan());
        assert!(m.avg_ttk_ms.is_nan());
        assert!(m.move_pct.is_nan());
        assert_eq!(m.on_target_pct, 0.0);
    }
}
