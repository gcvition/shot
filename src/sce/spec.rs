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

/// 运动规格（M1 填参数；`Static` 是唯一在 M0 用到的）。
#[derive(Clone, Debug, PartialEq)]
pub enum MotionSpec {
    Static,
    /// 直线来回（axis 归一化方向，amp 振幅米，hz 频率）。
    Strafe { axis: Vec3, amp: f32, hz: f32, phase_deg: f32 },
    /// 随机方向跳变。
    Dodge { speed: f32, interval: (f32, f32) },
    /// 沿路径点。
    Path { points: Vec<Vec3>, speed: f32, closed: bool },
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
