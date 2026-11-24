//! `.shot` v1 的只读兼容层。
//!
//! 只负责把旧文件（JSON 或裸 postcard）读进来，然后升级成 v2。
//! **不要** 再往 v1 写数据：新局永远写 v2。v1 的结构和磁盘字节一一对应，
//! 改字段顺序会直接弄坏旧回放。

use serde::{Deserialize, Serialize};

use crate::fov::FovKind;
use crate::settings::{DisplayMode, Settings};
use crate::vec3::Vec3;

/// 开局那一瞬间的设置快照。回放必须用当时的 FOV/分辨率，不能用现在的。
/// v2 也继续用这一份。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub cm_per_360: f64,
    pub dpi: u32,
    pub fov_deg: f32,
    pub fov_kind: FovKind,
    pub render_width: u32,
    pub render_height: u32,
    pub display_mode: DisplayMode,
    pub window_width: u32,
    pub window_height: u32,
    pub refresh_hz: u32,
}

impl SettingsSnapshot {
    pub fn from_settings(
        settings: &Settings,
        window_w: u32,
        window_h: u32,
        refresh_hz: u32,
    ) -> Self {
        Self {
            cm_per_360: settings.cm_per_360,
            dpi: settings.dpi,
            fov_deg: settings.fov_deg,
            fov_kind: settings.fov_kind,
            render_width: settings.render_width,
            render_height: settings.render_height,
            display_mode: settings.display_mode,
            window_width: window_w,
            window_height: window_h,
            refresh_hz,
        }
    }
}

/// v1 文件头。字段顺序对应旧 postcard 布局，不要改。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionHeaderV1 {
    pub schema_version: u16,
    pub session_id: String,
    #[serde(deserialize_with = "super::deserialize_scenario")]
    pub scenario: String,
    pub started_at_unix_ms: i64,
    pub duration_ms: u32,
    /// 开局 RNG。回放必须用同一颗种子才能复现随机墙。
    pub rng_seed: u64,
    pub settings: SettingsSnapshot,
}

/// 一次鼠标移动。`dx`/`dy` 是原始 count，yaw/pitch 是移动之后的朝向。
/// v1 和 v2 共用同一个结构。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct MouseSample {
    pub t_us: u64,
    pub dx: f32,
    pub dy: f32,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
}

/// v1 的开火记录。空枪时 `target_id` 为 0。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ShotEventV1 {
    pub t_us: u64,
    pub hit: bool,
    pub target_id: u32,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
}

/// v1 的世界事件只有 Spawn/Despawn 两种。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum WorldKindV1 {
    Spawn,
    Despawn,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WorldEventV1 {
    pub t_us: u64,
    pub kind: WorldKindV1,
    pub id: u32,
    pub pos: Vec3,
    pub radius: f32,
}

/// v1 的一整局轨迹。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionFileV1 {
    pub header: SessionHeaderV1,
    pub mouse: Vec<MouseSample>,
    pub shots: Vec<ShotEventV1>,
    pub world: Vec<WorldEventV1>,
}

impl SessionFileV1 {
    /// 升级到 v2。没有对应数据的字段给默认值，语义不变。
    pub fn into_v2(self) -> super::SessionFile {
        let header = super::SessionHeader {
            schema_version: super::SCHEMA_VERSION,
            session_id: self.header.session_id,
            scenario: self.header.scenario,
            started_at_unix_ms: self.header.started_at_unix_ms,
            duration_ms: self.header.duration_ms,
            rng_seed: self.header.rng_seed,
            settings: self.header.settings,
            scenario_hash: 0,
            sim_hz: 0,
            weapon_preset: String::new(),
            movement_preset: String::new(),
            difficulty: None,
            playlist_id: None,
            app_version: String::new(),
        };
        let shots = self
            .shots
            .into_iter()
            .map(|shot| super::ShotEvent {
                t_us: shot.t_us,
                hit: shot.hit,
                target_id: shot.target_id,
                yaw_deg: shot.yaw_deg,
                pitch_deg: shot.pitch_deg,
                spray_index: 0,
                punch_yaw_deg: 0.0,
                punch_pitch_deg: 0.0,
                spread_x: 0.0,
                spread_y: 0.0,
                player_vel: Vec3::ZERO,
                ammo_after: -1,
                hits: Vec::new(),
            })
            .collect();
        let world = self
            .world
            .into_iter()
            .map(|event| super::WorldEvent {
                t_us: event.t_us,
                kind: match event.kind {
                    WorldKindV1::Spawn => super::WorldKind::Spawn,
                    WorldKindV1::Despawn => super::WorldKind::Despawn,
                },
                id: event.id,
                pos: event.pos,
                radius: event.radius,
            })
            .collect();
        super::SessionFile {
            header,
            mouse: self.mouse,
            keys: Vec::new(),
            player: Vec::new(),
            view_punch: Vec::new(),
            shots,
            targets: Vec::new(),
            world,
            on_target: Vec::new(),
            damage_taken: Vec::new(),
            bots: Vec::new(),
            rounds: Vec::new(),
        }
    }
}
