//! 课程连播（M7）：把几个场景排成一条链，一局接一局打，中间是 `Intermission`。
//!
//! - [`BUILTIN`]：内置 benchmark 课程（Voltaic 风格：按技能分组的全项日课）。
//! - [`PlanRun`]：一局内的运行时资源。`tick_clock` 打完一关不退出，
//!   写完成绩后翻 `next`；`OnEnter(Intermission)` 的 `round_reset` 换 spec 重开。
//! - [`suggest_knobs`]：自适应难度——按该分类最近几局的命中率/跟枪率推
//!   速度/半径倍率，写进 `SessionRecord.difficulty` 留档。

use bevy::prelude::Resource;

use crate::sce::{ScenarioCategory, ScenarioSpec};
use crate::stats::StatsDb;

/// 一条课程 = 一串场景 id（按顺序打）。
pub struct Playlist {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub scenarios: &'static [&'static str],
}

pub const BUILTIN: &[Playlist] = &[
    Playlist {
        id: "voltaic-click",
        name: "VT 点击日课",
        description: "定位/甩枪/微调三项轮着来，经典 VT 结构。",
        scenarios: &[
            "Sixshot Ultimate",
            "Moving Clicks",
            "Wide Flick 1w3t",
            "Micro Precision",
            "Reflex Pop",
        ],
    },
    Playlist {
        id: "voltaic-track",
        name: "VT 跟枪日课",
        description: "平滑+变向+反应跟枪，按 VT Angelic 顺序。",
        scenarios: &[
            "Smooth Strafe Sphere",
            "Vertical Bounce",
            "Reactive ADAD",
            "Close Orbit",
            "Smooth Strafe Sphere",
        ],
    },
    Playlist {
        id: "voltaic-duel",
        name: "VT 对枪日课",
        description: "带 AI 还手的实战链，最后 1v3 收尾。",
        scenarios: &[
            "Reaction Duel",
            "ADAD Duel",
            "1v3 Arena",
        ],
    },
    Playlist {
        id: "full-day",
        name: "全项日课",
        description: "点击→跟枪→对枪一条龙，约 10 分钟。",
        scenarios: &[
            "Sixshot Ultimate",
            "Moving Clicks",
            "Smooth Strafe Sphere",
            "Reactive ADAD",
            "Reaction Duel",
        ],
    },
];

pub fn find(id: &str) -> Option<&'static Playlist> {
    BUILTIN.iter().find(|p| p.id == id || p.name == id)
}

/// 一局连播的运行时状态。`idx` 是「正在打/刚打完」的那关。
#[derive(Resource)]
pub struct PlanRun {
    pub playlist: &'static Playlist,
    pub idx: usize,
    /// 下一关的 spec（`round_reset` 消费掉）。
    pub next_spec: Option<ScenarioSpec>,
    /// 已完成的 (scenario_id, session_id, score)，结课写 plans 表用。
    pub results: Vec<(String, String, f64)>,
    /// 本关应用的难度倍率（落库留档）。
    pub difficulty: f32,
}

impl PlanRun {
    pub fn new(playlist: &'static Playlist) -> Self {
        Self {
            playlist,
            idx: 0,
            next_spec: None,
            results: Vec::new(),
            difficulty: 1.0,
        }
    }

    /// 打完当前关：记成绩，把 `next_spec` 装好就返回 true（还有下一关）。
    pub fn advance(
        &mut self,
        session_id: &str,
        score: f64,
        db: Option<&StatsDb>,
        specs: &[ScenarioSpec],
    ) -> bool {
        let done_id = self.playlist.scenarios[self.idx].to_string();
        self.results.push((done_id, session_id.to_string(), score));
        self.idx += 1;
        let Some(&next_id) = self.playlist.scenarios.get(self.idx) else {
            return false;
        };
        let Some(spec) = specs.iter().find(|s| s.id == next_id) else {
            return false;
        };
        let mut spec = spec.clone();
        self.difficulty = suggest_knobs(db, spec.category);
        apply_knobs(&mut spec, self.difficulty);
        self.next_spec = Some(spec);
        true
    }
}

/// 自适应倍率：分类最近 8 局平均命中率/跟枪率 >85% 加速，<50% 减速。
/// session_metrics 里 accuracy 与 on_target_pct 都是 0..1 分数，统一乘 100。
/// db 打不开 → 基准 1.0。
pub fn suggest_knobs(db: Option<&StatsDb>, category: Option<ScenarioCategory>) -> f32 {
    let Some(db) = db else { return 1.0 };
    let cat = category.map(|c| c.as_str()).unwrap_or("click");
    let acc = db
        .recent_metric(cat, "accuracy", 8)
        .or_else(|| db.recent_metric(cat, "on_target_pct", 8))
        .map(|a| a * 100.0);
    match acc {
        Some(a) if a >= 85.0 => 1.12,
        Some(a) if a >= 65.0 => 1.0,
        Some(a) if a >= 45.0 => 0.9,
        Some(_) => 0.8,
        None => 1.0,
    }
}

/// 把难度倍率落到 spec：速度区间放大 + 目标半径缩一点（更快更小）。
pub fn apply_knobs(spec: &mut ScenarioSpec, k: f32) {
    if (k - 1.0).abs() < f32::EPSILON {
        return;
    }
    for group in &mut spec.spawns {
        group.radius *= 2.0 - k; // 快目标配小点儿的球
        scale_motion(&mut group.motion, k);
    }
    spec.target_radius *= 2.0 - k;
}

fn scale_motion(m: &mut crate::sce::MotionSpec, k: f32) {
    use crate::sce::MotionSpec::*;
    let rng = |r: &mut (f32, f32)| {
        r.0 *= k;
        r.1 *= k;
    };
    match m {
        Strafe { speed, .. } => rng(speed),
        Linear { speed, .. } => rng(speed),
        Dodge(d) => {
            d.speed *= k;
            d.accel *= k;
        }
        Orbit { angular_speed, .. } => *angular_speed *= k,
        Sine { freq, .. } => *freq = freq.scale(k),
        SmoothRandom { max_speed, .. } => *max_speed *= k,
        Path { speed, .. } | Distance { speed, .. } => *speed *= k,
        Jumper { gravity, jump_speed, .. } => {
            *gravity *= k;
            *jump_speed *= k;
        }
        Composite(list) => {
            for sub in list {
                scale_motion(sub, k);
            }
        }
        _ => {}
    }
}
