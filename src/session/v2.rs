//! `.shot` v2：一局的完整轨迹（当前格式）。
//!
//! 和 v1 相比多了：按键、玩家位置采样、视角冲量、目标关键帧、在靶区间、
//! 受击、AI 调试、回合事件。静止场景里这些新向量会保持为空（或只写一条）。
//!
//! 序列化用 postcard（二进制）；写盘时再压一层 zstd，文件头是魔数 `SHOT2`。
//! 往已有结构里加字段要放在 **枚举/结构尾部**——postcard 按声明顺序编码，
//! 插在中间会让旧 v2 文件读歪。

use serde::{Deserialize, Serialize};

use super::v1::{MouseSample, SettingsSnapshot};
use crate::error::Result;
use crate::paths::AppPaths;
use crate::sce;
use crate::vec3::Vec3;

/// 当前写盘的 schema 版本。读代码永远要能读 1 和 2。
pub const SCHEMA_VERSION: u16 = 2;

/// v2 文件头的魔数。`load` 靠它分派解码路径。
pub const MAGIC_V2: &[u8; 5] = b"SHOT2";

/// v2 文件头 = v1 字段 + 复现一局所需的元数据。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionHeader {
    pub schema_version: u16,
    pub session_id: String,
    #[serde(deserialize_with = "super::deserialize_scenario")]
    pub scenario: String,
    pub started_at_unix_ms: i64,
    pub duration_ms: u32,
    /// 开局 RNG。回放必须用同一颗种子才能复现随机墙。
    pub rng_seed: u64,
    pub settings: SettingsSnapshot,
    /// 场景规格的指纹（M1 起填），用来发现「同 id 但内容改过」。
    pub scenario_hash: u64,
    /// 模拟频率（Hz）。0 = 本局没有固定步长模拟（M0 行为）。
    pub sim_hz: u16,
    /// `.sce` 的 ShotWeaponPreset / ShotMovementPreset。空串 = 站桩单击。
    pub weapon_preset: String,
    pub movement_preset: String,
    /// 自适应难度档位（M7）。None = 没开。
    pub difficulty: Option<f32>,
    /// 课程连播时属于哪个计划（M7）。
    pub playlist_id: Option<String>,
    /// 写这局的应用版本，回放不兼容时用它分辨。
    pub app_version: String,
}

/// 一次开火。`hit`/`target_id` 保留 v1 语义：空枪 `target_id` 为 0。
/// 连发/后坐力/扩散/弹匣字段在 M2 才填非默认值；`hits` 装所有命中（霰弹多发）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShotEvent {
    pub t_us: u64,
    pub hit: bool,
    pub target_id: u32,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    /// 本次连发里是第几发（半自动永远是 0）。
    pub spray_index: u16,
    /// 开火瞬间的视角冲量（后坐力）。
    pub punch_yaw_deg: f32,
    pub punch_pitch_deg: f32,
    /// 扩散抽样（相对准星的偏移，度）。
    pub spread_x: f32,
    pub spread_y: f32,
    /// 开火时玩家速度（米/秒）。
    pub player_vel: Vec3,
    /// 开火后弹匣余量。-1 = 场景没有弹匣概念。
    pub ammo_after: i16,
    /// 本枪命中的所有部位（霰弹/穿透可多条；空枪为空）。
    pub hits: Vec<ShotHit>,
}

/// 一条命中明细。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ShotHit {
    pub target_id: u32,
    /// [`HitPart`] 的判别值，存整数免得以后加部位炸兼容。
    pub part: u8,
    pub damage: f32,
    /// 命中时目标的渲染位置（所见即所得）。
    pub pos: Vec3,
}

/// 命中部位。`ShotHit::part` 用 `as u8` 存。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HitPart {
    Body = 0,
    Head = 1,
    Limb = 2,
}

/// 一次按键变化。`action` 是绑定名（forward/jump/crouch/reload/ads…），
/// 存字符串比存键码稳，换绑后旧轨迹还能看。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyEvent {
    pub t_us: u64,
    pub action: String,
    pub down: bool,
}

/// 每个模拟 tick 的玩家状态。静止场景只在开局写一条。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PlayerSample {
    pub t_us: u64,
    /// 脚底位置（米），眼睛 = pos + 站高。
    pub pos: Vec3,
    pub vel: Vec3,
    /// [`PlayerFlags`] 的位组合。
    pub flags: u8,
    pub hp: f32,
}

/// `PlayerSample::flags` 的位定义。
pub mod player_flags {
    pub const ON_GROUND: u8 = 1 << 0;
    pub const CROUCH: u8 = 1 << 1;
    pub const WALK: u8 = 1 << 2;
}

/// 视角冲量采样（后坐力把视线顶出去多少，度）。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PunchSample {
    pub t_us: u64,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
}

/// 目标关键帧：每秒 N tick 记一条 `id, pos, hp`。
/// 静止目标只在自己出生时写。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TargetKeyframe {
    pub t_us: u64,
    pub id: u32,
    pub pos: Vec3,
    pub hp: f32,
}

/// 跟枪场景里「准星在靶上」的一段区间。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct OnTargetSpan {
    pub start_us: u64,
    pub end_us: u64,
    pub target_id: u32,
}

/// AI 打中玩家的一次伤害。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct DamageEvent {
    pub t_us: u64,
    pub amount: f32,
    /// 伤害来源相对玩家的方位角（度），给受击指示器用。
    pub direction_yaw_deg: f32,
    pub source_id: u32,
}

/// AI 决策的调试快照（可在设置里关掉不写）。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BotDebugSample {
    pub t_us: u64,
    pub id: u32,
    /// 意图枚举（追击/急停/开火…）的判别值。
    pub intent: u8,
    pub aim_error_deg: f32,
    pub desired_yaw_deg: f32,
    pub desired_pitch_deg: f32,
}

/// 回合事件（对枪类场景）。`kind` 判别值留给 M6 定义。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RoundEvent {
    pub t_us: u64,
    pub kind: u8,
    pub round: u16,
}

/// 世界上目标的出现/消失/可见性。回放时按时间重放这张表就能重建现场。
/// 判别值顺序不能变：postcard 按序号编码，旧 v2 文件要靠它解码。
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorldKind {
    Spawn,
    Despawn,
    /// 被打死（区别于寿终/规则消失的 Despawn；分析/回放要区分）。
    Killed,
    /// 从掩体/墙后变可见（Peek 场景，M4 起写）。
    Visible,
    Hidden,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WorldEvent {
    pub t_us: u64,
    pub kind: WorldKind,
    pub id: u32,
    pub pos: Vec3,
    pub radius: f32,
}

/// 一整局的轨迹。存 `cache/sessions/<session_id>.shot`。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionFile {
    pub header: SessionHeader,
    pub mouse: Vec<MouseSample>,
    pub keys: Vec<KeyEvent>,
    pub player: Vec<PlayerSample>,
    pub view_punch: Vec<PunchSample>,
    pub shots: Vec<ShotEvent>,
    pub targets: Vec<TargetKeyframe>,
    pub world: Vec<WorldEvent>,
    pub on_target: Vec<OnTargetSpan>,
    pub damage_taken: Vec<DamageEvent>,
    pub bots: Vec<BotDebugSample>,
    pub rounds: Vec<RoundEvent>,
}

impl SessionFile {
    pub fn new(
        session_id: String,
        scenario: impl Into<String>,
        rng_seed: u64,
        settings: SettingsSnapshot,
    ) -> Self {
        Self {
            header: SessionHeader {
                schema_version: SCHEMA_VERSION,
                session_id,
                scenario: sce::migrate_scenario_id(&scenario.into()),
                started_at_unix_ms: chrono::Utc::now().timestamp_millis(),
                duration_ms: 0,
                rng_seed,
                settings,
                scenario_hash: 0,
                sim_hz: 0,
                weapon_preset: String::new(),
                movement_preset: String::new(),
                difficulty: None,
                playlist_id: None,
                app_version: crate::VERSION.to_string(),
            },
            mouse: Vec::with_capacity(64_000),
            keys: Vec::new(),
            player: Vec::new(),
            view_punch: Vec::new(),
            shots: Vec::with_capacity(512),
            targets: Vec::new(),
            world: Vec::with_capacity(512),
            on_target: Vec::new(),
            damage_taken: Vec::new(),
            bots: Vec::new(),
            rounds: Vec::new(),
        }
    }

    /// 写盘：`SHOT2` + zstd(postcard)。一次写完，不在对局中调用。
    pub fn save(&self, paths: &AppPaths) -> Result<()> {
        paths.ensure_dirs()?;
        let bytes = super::codec::encode_v2(self)?;
        // 原子写：tmp + rename，崩溃不留半截文件。
        let path = paths.session_trace(&self.header.session_id);
        let tmp = path.with_extension("shot.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// 读 `.shot`：v2 二进制 / v1 JSON / v1 postcard 都能读，统一升级成 v2。
    pub fn load(paths: &AppPaths, session_id: &str) -> Result<Self> {
        let path = paths.session_trace(session_id);
        let bytes = std::fs::read(&path)?;
        let mut file = super::codec::decode(&bytes)?.into_v2();
        if file.header.schema_version > SCHEMA_VERSION {
            return Err(crate::error::ShotError::Other(format!(
                "session schema {} 比当前支持的 {SCHEMA_VERSION} 新",
                file.header.schema_version
            )));
        }
        // 文件名是权威 id；内容里不一致时按文件名归一。
        if file.header.session_id != session_id {
            file.header.session_id = session_id.to_string();
        }
        Ok(file)
    }

    /// 按时间取视角。没有采样时返回 (0, 0)。
    pub fn look_at(&self, t_us: u64) -> (f32, f32) {
        if self.mouse.is_empty() {
            return (0.0, 0.0);
        }
        match self.mouse.binary_search_by(|s| s.t_us.cmp(&t_us)) {
            Ok(i) => (self.mouse[i].yaw_deg, self.mouse[i].pitch_deg),
            Err(0) => (self.mouse[0].yaw_deg, self.mouse[0].pitch_deg),
            Err(i) if i >= self.mouse.len() => {
                let last = self.mouse.last().expect("mouse not empty");
                (last.yaw_deg, last.pitch_deg)
            }
            Err(i) => {
                let a = &self.mouse[i - 1];
                let b = &self.mouse[i];
                let span = b.t_us.saturating_sub(a.t_us).max(1) as f32;
                let t = (t_us.saturating_sub(a.t_us)) as f32 / span;
                // yaw 走最短角向插值：179.9 → -179.9 跨 ±180 不许绕 360。
                let dyaw = (b.yaw_deg - a.yaw_deg + 540.0) % 360.0 - 180.0;
                (
                    (a.yaw_deg + dyaw * t + 540.0) % 360.0 - 180.0,
                    a.pitch_deg + (b.pitch_deg - a.pitch_deg) * t,
                )
            }
        }
    }
}
