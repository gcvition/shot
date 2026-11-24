//! 场景规约 v2（M0 骨架）。
//!
//! [`ScenarioSpec`] 是菜单和 Bevy 子进程之间的唯一数据通道：菜单把
//! `.sce` 解析成它，写进启动参数传给 `shot viewport`。v1 场景（不带
//! `[Shot Extension]` 的 KovaaK 格式）全部走 `legacy` 字段归一化，
//! 视口按 v1 的球墙/网格逻辑跑，保证行为完全一致。

use crate::vec3::Vec3;

#[derive(Clone, Debug, PartialEq)]
pub struct ScenarioSpec {
    /// 场景 id（文件名去后缀）。
    pub id: String,
    /// 菜单显示的友好名。
    pub display_name: String,
    /// 场景描述（菜单 tooltip / 详情页）。
    pub description: String,
    /// 检索用标签（游戏 / 瞄准类型 / 难度 / 技能权重）。
    pub tags: ScenarioTags,
    /// KovaaK 式技能分类；None 表示未标注。
    pub category: Option<ScenarioCategory>,
    /// 回合时长（秒）。
    pub duration_secs: f32,
    /// 结束条件；时长到点后也会截停。
    pub end: EndCondition,
    /// 坐标单位约定（legacy = 米，unreal = 厘米）。
    pub units: UnitMode,
    pub player: PlayerSpec,
    pub weapon: WeaponSpec,
    /// 归一化后的出生组；legacy 场景为空。
    pub spawns: Vec<SpawnGroup>,
    pub scoring: ScoringSpec,
    pub map: MapSpec,
    pub bots: Vec<BotSpec>,
    /// 自适应难度的可调参数范围。
    pub difficulty: DifficultyKnobs,
    /// 目标材质是不是高光球（v1 网格场景用）。
    pub glossy: bool,
    /// 同屏目标上限（legacy 字段，从 grid cells 或场景解析归一化）。
    pub live_count: usize,
    /// 目标半径（米，legacy 字段）。
    pub target_radius: f32,
    /// 旧版生成方式：随机球墙或 NxN 网格。
    /// 有值 = v1 场景，视口按旧逻辑跑；None = v2 DSL 场景。
    pub legacy: Option<LegacySpawn>,
}

impl ScenarioSpec {
    /// `export-trace` 回放、CLI `--scenario <id>` 兜底时用。
    /// 产生一个和 Sixshot 等价的 legacy 场景，保证回放行为可复现。
    pub fn fallback(id: &str) -> Self {
        Self {
            id: id.to_string(),
            display_name: id.to_string(),
            description: String::new(),
            tags: ScenarioTags::default(),
            category: None,
            duration_secs: 60.0,
            end: EndCondition::Duration,
            units: UnitMode::Legacy,
            player: PlayerSpec::default(),
            weapon: WeaponSpec::hitscan_click(),
            spawns: Vec::new(),
            scoring: ScoringSpec::legacy_hits(),
            map: MapSpec::default(),
            bots: Vec::new(),
            difficulty: DifficultyKnobs::default(),
            glossy: false,
            live_count: 6,
            target_radius: 0.13,
            legacy: Some(LegacySpawn::RandomWall),
        }
    }
}

/// 旧版目标生成方式（v1 `.sce`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacySpawn {
    /// 在面前弧形墙上随机散布 `live_count` 个球（KovaaK 球墙）。
    RandomWall,
    /// N×N 网格，正中偏上（Gridshot 系）。
    Grid { cells: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitMode {
    /// 米、度 / 秒（旧约定，KovaaK 球墙就是按这个调的）。
    Legacy,
    /// Unreal 单位（厘米），`[Shot Extension] ShotUnits=unreal` 时启用。
    Unreal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EndCondition {
    /// 到时长结束。
    Duration,
    /// 击杀数达标提前结束。
    Kills(u32),
    /// 累计伤害达标提前结束。
    Damage(f32),
    /// 打满 N 回合（每回合独立计时）。
    Rounds(u32),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScenarioTags {
    pub game: Option<String>,
    pub aim_type: Option<String>,
    pub difficulty: Option<String>,
    /// (skill_id, weight)，权重和为 1。
    pub skill_weights: Vec<(String, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenarioCategory {
    Flick,
    Tracking,
    Switching,
    Recoil,
    Movement,
    Peek,
    Reaction,
    Duel,
    Benchmark,
}

impl ScenarioCategory {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "flick" | "clicking" | "click" => Some(Self::Flick),
            "tracking" | "track" => Some(Self::Tracking),
            "switching" | "switch" => Some(Self::Switching),
            "recoil" | "spray" | "control" => Some(Self::Recoil),
            "movement" | "strafe" => Some(Self::Movement),
            "peek" => Some(Self::Peek),
            "reaction" | "reactive" => Some(Self::Reaction),
            "duel" | "1v1" => Some(Self::Duel),
            "benchmark" | "bench" => Some(Self::Benchmark),
            _ => None,
        }
    }

    /// 写进 `stats.db` / 课程配置的稳定字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flick => "flick",
            Self::Tracking => "tracking",
            Self::Switching => "switching",
            Self::Recoil => "recoil",
            Self::Movement => "movement",
            Self::Peek => "peek",
            Self::Reaction => "reaction",
            Self::Duel => "duel",
            Self::Benchmark => "benchmark",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerSpec {
    /// 起点（脚底）。
    pub start_pos: Vec3,
    /// 初始朝向。
    pub start_yaw_deg: f32,
    pub hp: f32,
    pub movement_preset: MovementPreset,
}

impl Default for PlayerSpec {
    fn default() -> Self {
        Self {
            start_pos: Vec3::ZERO,
            start_yaw_deg: 0.0,
            hp: 100.0,
            movement_preset: MovementPreset::Static,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovementPreset {
    /// 站桩（全部 v1 场景、纯点击训练）。
    Static,
    /// CS / Valorant 式：急停收益大、peek 慢、可蹲。
    Tactical,
    /// Quake / Apex 式：高机动、滑铲、空中控制。
    Arena,
    /// 大逃杀式：中速、跳跃惩罚中等。
    BattleRoyale,
}

impl MovementPreset {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "static" | "none" => Some(Self::Static),
            "tactical" | "cs" | "val" | "valorant" => Some(Self::Tactical),
            "arena" | "quake" | "apex" => Some(Self::Arena),
            "battleroyale" | "br" => Some(Self::BattleRoyale),
            _ => None,
        }
    }
}

/// 武器参数。`hitscan_click()` 是 v1 的单发即点即中，
/// 其余字段是 M2+ 武器系统用的默认值占位。
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponSpec {
    pub kind: WeaponKind,
    pub fire_mode: FireMode,
    /// 最小开火间隔（秒）；0 = 不限（点击多快打多快）。
    pub fire_interval: f32,
    pub damage: f32,
    pub headshot_mult: f32,
    /// 弹丸数（霰弹）。
    pub pellets: u32,
    /// 弹匣容量；None = 无限。
    pub magazine: Option<u32>,
    /// 换弹时长（打空 / 战术换弹）。
    pub reload_empty: f32,
    pub reload_partial: f32,
    /// 伤害衰减；(start_m, stop_m, min_factor)。
    pub falloff: Option<(f32, f32, f32)>,
    pub spread: SpreadSpec,
    pub recoil: RecoilSpec,
    pub ads: Option<AdsSpec>,
}

impl WeaponSpec {
    pub fn hitscan_click() -> Self {
        Self {
            kind: WeaponKind::Hitscan,
            fire_mode: FireMode::Semi,
            fire_interval: 0.0,
            damage: 0.0,
            headshot_mult: 1.0,
            pellets: 1,
            magazine: None,
            reload_empty: 0.0,
            reload_partial: 0.0,
            falloff: None,
            spread: SpreadSpec::default(),
            recoil: RecoilSpec::default(),
            ads: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WeaponKind {
    Hitscan,
    /// 射线 / 持续伤害光束。
    Beam,
    /// 抛射物。
    Projectile { speed: f32, gravity_scale: f32, radius: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FireMode {
    /// 单发，按住不连发。
    Semi,
    /// 按住连发。
    Auto,
    /// 点射（n 发一组）。
    Burst { count: u32, interval: f32 },
}

/// 扩散参数（基础值, 每发增长, 衰减/秒, 上限），单位度。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpreadSpec {
    pub base: f32,
    pub per_shot: f32,
    pub decay: f32,
    pub max: f32,
}

/// 后坐力（M2）。pattern 指向 `res/recoil/<name>.json`。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecoilSpec {
    pub pattern: Option<String>,
    /// 视角跟随后坐力的比例（弹道=1，CS 约 0.5）。
    pub view_scale: f32,
    /// 停火后每秒回退的度数。
    pub reset_dps: f32,
    /// 鼠标下拉是否直接抵消累积（Apex/OW 语义）。
    pub negatable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdsSpec {
    pub zoom_fov: f32,
    pub sens_mult: f32,
    pub time_in: f32,
    pub blocks_shooting: bool,
}

/// 一个出生组：什么目标、几个、在哪出、怎么动。
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnGroup {
    pub archetype: TargetShape,
    /// 命中半径（米）；0 = 用 spec.target_radius。
    pub radius: f32,
    /// true = 不该打的目标（Go/No-Go）：命中算惩罚。
    pub no_shoot: bool,
    pub count: usize,
    pub region: SpawnRegion,
    pub motion: MotionSpec,
    pub rule: SpawnRule,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TargetShape {
    Sphere,
    Humanoid,
    Box,
    Disc,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SpawnRegion {
    /// 正对玩家的矩形墙。
    WallRect { center: Vec3, size: Vec3 },
    /// 离散网格点。
    Grid { origin: Vec3, cols: u32, rows: u32, spacing: Vec3 },
    /// 体素盒内均匀随机。
    Box { min: Vec3, max: Vec3 },
    /// 以玩家为中心的角壳（yaw/pitch 范围 + 距离范围）。
    AngularShell {
        yaw_range: (f32, f32),
        pitch_range: (f32, f32),
        dist_range: (f32, f32),
    },
    /// 固定点集。
    Points(Vec<SpawnPoint>),
    /// 掩体边缘（M4+）。
    CoverEdges { cover_ids: Vec<u32>, side: i8, head_height: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnPoint {
    pub pos: Vec3,
    pub yaw_deg: f32,
    pub weight: f32,
}

/// 出生约束。
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnRule {
    /// 目标之间的最小间距（米）。
    pub min_separation: f32,
    /// 距离准星的最小 / 最大夹角（度）。
    pub min_angle_from_crosshair_deg: f32,
    pub max_angle_from_crosshair_deg: f32,
    /// 上目标死到下目标出生的延迟（秒，min-max 随机）。
    pub delay: (f32, f32),
    /// 存活时长（秒，min-max 随机）；None = 打到死。
    pub lifetime: Option<(f32, f32)>,
    /// 同屏上限。
    pub max_alive: usize,
    /// 出生必须看得见（无遮挡）。
    pub require_los: bool,
}

impl Default for SpawnRule {
    fn default() -> Self {
        Self {
            min_separation: 0.0,
            min_angle_from_crosshair_deg: 0.0,
            max_angle_from_crosshair_deg: 180.0,
            delay: (0.0, 0.0),
            lifetime: None,
            max_alive: usize::MAX,
            require_los: false,
        }
    }
}

/// 运动规格 DSL（`ShotMotion=` 和 `.sce` Dodge Profile 归一化后的产物）。
/// `(f32, f32)` 参数都是区间；生成目标时用该目标的派生 RNG 采样。
#[derive(Clone, Debug, PartialEq)]
pub enum MotionSpec {
    Static,
    /// 匀速直线（速度、水平方向角范围度）。
    Linear { speed: (f32, f32), dir_deg: (f32, f32) },
    /// 左右横移。
    Strafe {
        speed: (f32, f32),
        interval: (f32, f32),
        accel: f32,
        /// 是否对称（对称 = 初始方向随机±，非对称永远从同一侧开始）。
        symmetric: bool,
    },
    /// 正弦（每轴振幅 / 频率 / 相位）。
    Sine { amp: Vec3, freq: Vec3, phase: Vec3 },
    /// 绕玩家或定点转圈。
    Orbit {
        radius: f32,
        angular_speed: f32,
        /// 绕玩家 = None。
        center: Option<Vec3>,
        y_offset: f32,
    },
    /// Ornstein-Uhlenbeck 平滑随机漂移。
    SmoothRandom { max_speed: f32, tau: f32 },
    /// KovaaK Dodge Profile 等价（见 [`DodgeSpec`]）。
    Dodge(DodgeSpec),
    /// 垂直起落。
    Jumper {
        jump_speed: f32,
        gravity: f32,
        interval: (f32, f32),
    },
    /// 瞬移。
    Blink {
        interval: (f32, f32),
        dist: (f32, f32),
    },
    /// 沿航点。
    Path {
        points: Vec<Vec3>,
        speed: f32,
        closed: bool,
    },
    /// 与玩家保持距离带。
    Distance { band: (f32, f32), speed: f32 },
    /// 叠加。
    Composite(Vec<MotionSpec>),
}

/// `ShotMotion=Dodge ...` 或 `.sce` `[Dodge Profile]` 的参数。
#[derive(Clone, Debug, PartialEq)]
pub struct DodgeSpec {
    pub speed: f32,
    pub accel: f32,
    pub gravity: f32,
    pub jump_speed: f32,
    pub lr_interval: (f32, f32),
    pub fb_interval: (f32, f32),
    pub jump_interval: (f32, f32),
    pub dist_band: (f32, f32),
    /// 被打后变向概率与延迟。
    pub react_chance: f32,
    pub react_delay: (f32, f32),
    pub react_cooldown: f32,
}

impl Default for DodgeSpec {
    fn default() -> Self {
        Self {
            speed: 3.0,
            accel: 30.0,
            gravity: 20.0,
            jump_speed: 6.0,
            lr_interval: (0.5, 1.5),
            fb_interval: (0.8, 2.0),
            jump_interval: (1.0, 3.0),
            dist_band: (6.0, 16.0),
            react_chance: 0.5,
            react_delay: (0.1, 0.35),
            react_cooldown: 0.3,
        }
    }
}

impl MotionSpec {
    /// 生成该目标的运行时 [`Motion`](crate::sim::motion::Motion)。
    /// `anchor` = 出生点；`rng` = 该目标自己的派生 RNG（区间采样在这里定死）。
    pub fn to_motion(
        &self,
        anchor: Vec3,
        rng: &mut rand_chacha::ChaCha8Rng,
    ) -> crate::sim::motion::Motion {
        use crate::sim::motion::{DodgeState, Motion};
        use rand::Rng;
        fn range(rng: &mut rand_chacha::ChaCha8Rng, (lo, hi): (f32, f32)) -> f32 {
            if hi <= lo {
                lo
            } else {
                lo + rng.random::<f32>() * (hi - lo)
            }
        }
        match self {
            Self::Static => Motion::Static,
            Self::Linear { speed, dir_deg } => {
                let yaw = range(rng, *dir_deg).to_radians();
                Motion::Linear {
                    speed: range(rng, *speed),
                    dir: Vec3::new(yaw.cos(), 0.0, yaw.sin()),
                }
            }
            Self::Strafe {
                speed,
                interval,
                accel,
                symmetric,
            } => {
                let dir = if *symmetric || rng.random::<bool>() {
                    1.0
                } else {
                    -1.0
                };
                Motion::Strafe {
                    speed: range(rng, *speed),
                    interval: *interval,
                    accel: *accel,
                    axis: Vec3::new(1.0, 0.0, 0.0),
                    dir,
                    next_flip: range(rng, *interval),
                    t: 0.0,
                }
            }
            Self::Sine { amp, freq, phase } => Motion::Sine {
                anchor,
                amp: *amp,
                freq: *freq,
                phase: *phase,
            },
            Self::Orbit {
                radius,
                angular_speed,
                center,
                y_offset,
            } => Motion::Orbit {
                center: *center,
                radius: *radius,
                angular_speed: *angular_speed,
                angle: rng.random::<f32>() * std::f32::consts::TAU,
                y_offset: *y_offset,
            },
            Self::SmoothRandom { max_speed, tau } => Motion::SmoothRandom {
                max_speed: *max_speed,
                tau: *tau,
            },
            Self::Dodge(d) => Motion::Dodge(Box::new(DodgeState {
                speed: d.speed,
                accel: d.accel,
                gravity: d.gravity,
                jump_speed: d.jump_speed,
                lr_interval: d.lr_interval,
                fb_interval: d.fb_interval,
                jump_interval: d.jump_interval,
                dist_band: d.dist_band,
                react_chance: d.react_chance,
                react_delay: d.react_delay,
                react_cooldown: d.react_cooldown,
                ground_y: anchor.y,
                next_lr: range(rng, d.lr_interval),
                next_fb: range(rng, d.fb_interval),
                next_jump: range(rng, d.jump_interval),
                ..Default::default()
            })),
            Self::Jumper {
                jump_speed,
                gravity,
                interval,
            } => Motion::Jumper {
                jump_speed: *jump_speed,
                gravity: *gravity,
                interval: *interval,
                ground_y: anchor.y,
                next_jump: range(rng, *interval),
                t: 0.0,
            },
            Self::Blink { interval, dist } => Motion::Blink {
                interval: *interval,
                dist: *dist,
                next_blink: range(rng, *interval),
                t: 0.0,
            },
            Self::Path {
                points,
                speed,
                closed,
            } => Motion::Path {
                points: points.clone(),
                speed: *speed,
                closed: *closed,
                index: 0,
            },
            Self::Distance { band, speed } => Motion::Distance {
                band: *band,
                speed: *speed,
            },
            Self::Composite(parts) => {
                Motion::Composite(parts.iter().map(|p| p.to_motion(anchor, rng)).collect())
            }
        }
    }
}

impl SpawnRegion {
    /// 在区域里采一个点。`eye` = 玩家眼睛（AngularShell 用）。
    pub fn sample(&self, rng: &mut rand_chacha::ChaCha8Rng, eye: Vec3) -> Vec3 {
        use rand::Rng;
        fn range(rng: &mut rand_chacha::ChaCha8Rng, (lo, hi): (f32, f32)) -> f32 {
            if hi <= lo {
                lo
            } else {
                lo + rng.random::<f32>() * (hi - lo)
            }
        }
        match self {
            Self::WallRect { center, size } => Vec3::new(
                center.x + (rng.random::<f32>() - 0.5) * size.x,
                center.y + (rng.random::<f32>() - 0.5) * size.y,
                center.z + (rng.random::<f32>() - 0.5) * size.z,
            ),
            Self::Grid {
                origin,
                cols,
                rows,
                spacing,
            } => {
                let i = rng.random_range(0..(*cols * *rows).max(1));
                Vec3::new(
                    origin.x + (i % *cols) as f32 * spacing.x,
                    origin.y + (i / *cols) as f32 * spacing.y,
                    origin.z + (i % *cols) as f32 * spacing.z,
                )
            }
            Self::Box { min, max } => Vec3::new(
                range(rng, (min.x, max.x)),
                range(rng, (min.y, max.y)),
                range(rng, (min.z, max.z)),
            ),
            Self::AngularShell {
                yaw_range,
                pitch_range,
                dist_range,
            } => {
                let yaw = range(rng, *yaw_range).to_radians();
                let pitch = range(rng, *pitch_range).to_radians();
                let dist = range(rng, *dist_range);
                Vec3::new(
                    eye.x + dist * pitch.cos() * yaw.sin(),
                    eye.y + dist * pitch.sin(),
                    eye.z - dist * pitch.cos() * yaw.cos(),
                )
            }
            Self::Points(points) => {
                if points.is_empty() {
                    return Vec3::ZERO;
                }
                let total: f32 = points.iter().map(|p| p.weight.max(0.0)).sum();
                if total <= 0.0 {
                    return points[0].pos;
                }
                let mut pick = rng.random::<f32>() * total;
                for p in points {
                    pick -= p.weight.max(0.0);
                    if pick <= 0.0 {
                        return p.pos;
                    }
                }
                points[points.len() - 1].pos
            }
            Self::CoverEdges { .. } => Vec3::ZERO, // M4 才有掩体
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScoringMode {
    /// v1：命中 +1、空枪 -1。
    LegacyHits,
    /// KovaaK 通用计分（伤害 + 击杀 + 命中率系数）。
    Kovaak,
    /// 跟枪：按准星在目标上的时间占比。
    OnTargetPercent,
    /// 反应：奖励反应时间。
    ReactionMs,
    /// 表达式计分（M7）。
    Custom,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScoringSpec {
    pub mode: ScoringMode,
    /// KovaaK 系数：每点伤害、每杀、每秒存活的分值。
    pub per_damage: f32,
    pub per_kill: f32,
    pub per_time: f32,
    /// 惩罚：空枪、受伤、死亡。
    pub loss_per_miss: f32,
    pub loss_per_damage_taken: f32,
    pub loss_per_death: f32,
    /// 是否乘命中率 / sqrt(命中率)。
    pub mult_accuracy: bool,
    pub mult_sqrt_acc: bool,
}

impl ScoringSpec {
    pub fn legacy_hits() -> Self {
        Self {
            mode: ScoringMode::LegacyHits,
            per_damage: 1.0,
            per_kill: 1.0,
            per_time: 0.0,
            loss_per_miss: 1.0,
            loss_per_damage_taken: 0.0,
            loss_per_death: 0.0,
            mult_accuracy: false,
            mult_sqrt_acc: false,
        }
    }
}

/// 地图（M3）。v1 场景用默认的 KovaaK 白墙房间。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapSpec {
    /// 房间 AABB（中心 / 半尺寸）；None = v1 白房间。
    pub bounds: Option<(Vec3, Vec3)>,
    /// 障碍块。
    pub blocks: Vec<Block>,
    /// 玩家重生点。
    pub player_spawns: Vec<SpawnPoint>,
    /// 目标出生点集（region 里引用）。
    pub points: Vec<SpawnPoint>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub min: Vec3,
    pub max: Vec3,
}

/// AI 对手 / 假人（M6）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BotSpec {
    pub name: String,
    pub aim_profile: Option<String>,
    /// 行为标签（如 "peek", "wide_swing"）。
    pub behavior: Option<String>,
}

/// 自适应难度可调参数：(字段路径, min, max)。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DifficultyKnobs {
    pub ranges: Vec<(String, f32, f32)>,
}
