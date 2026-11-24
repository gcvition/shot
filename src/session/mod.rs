//! 一局训练的完整轨迹（`.shot` 文件）。
//!
//! 里面有三类时间序列，时间单位都是 **微秒**（`t_us`）：
//! - [`MouseSample`]：每次鼠标移动后的 yaw/pitch
//! - [`ShotEvent`]：每一次开火（v2 起含连发序号、扩散、弹匣等）
//! - [`WorldEvent`]：目标出现 / 消失 / 可见性变化
//!
//! v2 起还有按键、玩家位置、视角冲量、目标关键帧等向量；回放时
//! [`SessionFile::look_at`] 在相邻两个鼠标采样之间做线性插值。
//!
//! # 磁盘格式
//!
//! | 格式 | 识别 | 写入 |
//! |------|------|------|
//! | v2 `SHOT2` + zstd(postcard) | 魔数 | 当前唯一写格式 |
//! | v1 JSON `{...}` | 首个非空白字符 | 不再写 |
//! | v1 裸 postcard | 兜底 | 不再写 |
//!
//! 旧文件**只读不回写**（v1 时代会把 postcard 转存成 JSON，这个行为已去掉）。
//! 调试想用人眼看的格式：`shot.exe export-trace <id>` 导出 JSON 到 stdout。

mod codec;
mod v1;
mod v2;

pub use codec::{SessionAny, decode, encode_v2};
pub use v1::{MouseSample, SettingsSnapshot};
pub use v2::{
    BotDebugSample, DamageEvent, HitPart, KeyEvent, MAGIC_V2, OnTargetSpan, PlayerSample,
    PunchSample, RoundEvent, SCHEMA_VERSION, SessionFile, SessionHeader, ShotEvent, ShotHit,
    TargetKeyframe, WorldEvent, WorldKind, player_flags,
};

// v1 兼容类型只在 codec 内部用，不向外暴露。
use serde::Deserialize;

/// 旧存档 / 旧 `.shot` 里的枚举式场景 id 归一成现在的文件名。
pub(crate) fn deserialize_scenario<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    let raw = String::deserialize(deserializer)?;
    Ok(crate::sce::migrate_scenario_id(&raw))
}

#[cfg(test)]
mod tests {
    use super::v1::{SessionFileV1, SessionHeaderV1, WorldKindV1};
    use super::*;
    use crate::fov::FovKind;
    use crate::paths::AppPaths;
    use crate::settings::{DisplayMode, Settings};

    fn paths_in(dir: &std::path::Path) -> AppPaths {
        let paths = AppPaths::from_root(dir.to_path_buf());
        paths.ensure_dirs().unwrap();
        paths
    }

    fn snapshot() -> SettingsSnapshot {
        SettingsSnapshot::from_settings(&Settings::default(), 1920, 1080, 144)
    }

    #[test]
    fn v2_roundtrip_should_preserve_all_fields() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let mut file = SessionFile::new("s1".into(), "SixTargets", 42, snapshot());
        file.mouse.push(MouseSample {
            t_us: 0,
            dx: 1.0,
            dy: 0.0,
            yaw_deg: 0.5,
            pitch_deg: 0.0,
        });
        file.shots.push(ShotEvent {
            t_us: 1_500,
            hit: true,
            target_id: 3,
            yaw_deg: 1.25,
            pitch_deg: -0.5,
            spray_index: 0,
            punch_yaw_deg: 0.0,
            punch_pitch_deg: 0.0,
            spread_x: 0.0,
            spread_y: 0.0,
            player_vel: crate::vec3::Vec3::ZERO,
            ammo_after: -1,
            hits: vec![ShotHit {
                target_id: 3,
                part: HitPart::Body as u8,
                damage: 100.0,
                pos: crate::vec3::Vec3::new(0.5, 2.0, -7.15),
            }],
        });
        file.world.push(WorldEvent {
            t_us: 0,
            kind: WorldKind::Spawn,
            id: 3,
            pos: crate::vec3::Vec3::new(0.5, 2.0, -7.15),
            radius: 0.05,
        });
        file.save(&paths).unwrap();

        let raw = std::fs::read(paths.session_trace("s1")).unwrap();
        assert!(raw.starts_with(MAGIC_V2));

        let loaded = SessionFile::load(&paths, "s1").unwrap();
        assert_eq!(loaded.header.schema_version, SCHEMA_VERSION);
        assert_eq!(loaded.header.rng_seed, 42);
        assert_eq!(loaded.mouse.len(), 1);
        assert!((loaded.mouse[0].yaw_deg - 0.5).abs() < 1e-6);
        assert_eq!(loaded.shots.len(), 1);
        assert_eq!(loaded.shots[0].hits.len(), 1);
        assert_eq!(loaded.shots[0].hits[0].part, HitPart::Body as u8);
        assert!(matches!(loaded.world[0].kind, WorldKind::Spawn));
        assert_eq!(loaded.header.settings.fov_kind, FovKind::HorizontalRes);
        assert_eq!(
            loaded.header.settings.display_mode,
            DisplayMode::BorderlessStretch
        );
        assert_eq!(loaded.header.app_version, crate::VERSION);
    }

    #[test]
    fn v1_json_should_load_and_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let file = SessionFileV1 {
            header: SessionHeaderV1 {
                schema_version: 1,
                session_id: "old".into(),
                scenario: "six-targets".into(),
                started_at_unix_ms: 1_000,
                duration_ms: 60_000,
                rng_seed: 7,
                settings: snapshot(),
            },
            mouse: vec![MouseSample {
                t_us: 0,
                dx: 0.0,
                dy: 0.0,
                yaw_deg: 0.0,
                pitch_deg: 0.0,
            }],
            shots: vec![super::v1::ShotEventV1 {
                t_us: 5_000,
                hit: true,
                target_id: 2,
                yaw_deg: 0.1,
                pitch_deg: 0.2,
            }],
            world: vec![super::v1::WorldEventV1 {
                t_us: 0,
                kind: WorldKindV1::Spawn,
                id: 2,
                pos: crate::vec3::Vec3::new(0.0, 2.0, -7.15),
                radius: 0.05,
            }],
        };
        std::fs::write(
            paths.session_trace("old"),
            serde_json::to_vec_pretty(&file).unwrap(),
        )
        .unwrap();

        let loaded = SessionFile::load(&paths, "old").unwrap();
        assert_eq!(loaded.header.schema_version, SCHEMA_VERSION);
        assert_eq!(loaded.header.scenario, "Sixshot Ultimate");
        assert_eq!(loaded.header.duration_ms, 60_000);
        assert_eq!(loaded.shots.len(), 1);
        assert!(loaded.shots[0].hit);
        assert_eq!(loaded.shots[0].target_id, 2);
        assert!(matches!(loaded.world[0].kind, WorldKind::Spawn));
        // v1 不会回写成新格式，磁盘文件必须原样保留。
        let raw = std::fs::read_to_string(paths.session_trace("old")).unwrap();
        assert!(raw.trim_start().starts_with('{'));
    }

    #[test]
    fn v1_postcard_should_load_and_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let file = SessionFileV1 {
            header: SessionHeaderV1 {
                schema_version: 1,
                session_id: "pc".into(),
                scenario: "GridShot".into(),
                started_at_unix_ms: 2_000,
                duration_ms: 45_000,
                rng_seed: 9,
                settings: snapshot(),
            },
            mouse: Vec::new(),
            shots: Vec::new(),
            world: Vec::new(),
        };
        std::fs::write(
            paths.session_trace("pc"),
            postcard::to_allocvec(&file).unwrap(),
        )
        .unwrap();

        let loaded = SessionFile::load(&paths, "pc").unwrap();
        assert_eq!(loaded.header.schema_version, SCHEMA_VERSION);
        assert_eq!(loaded.header.scenario, "Gridshot Ultimate");
        assert_eq!(loaded.header.rng_seed, 9);
    }

    #[test]
    fn look_at_should_interpolate_between_samples() {
        let mut file = SessionFile::new("t".into(), "Sixshot Ultimate", 1, snapshot());
        file.mouse.push(MouseSample {
            t_us: 0,
            dx: 0.0,
            dy: 0.0,
            yaw_deg: 0.0,
            pitch_deg: 0.0,
        });
        file.mouse.push(MouseSample {
            t_us: 10_000,
            dx: 0.0,
            dy: 0.0,
            yaw_deg: 10.0,
            pitch_deg: -4.0,
        });
        let (yaw, pitch) = file.look_at(5_000);
        assert!((yaw - 5.0).abs() < 1e-4);
        assert!((pitch + 2.0).abs() < 1e-4);
    }
}
