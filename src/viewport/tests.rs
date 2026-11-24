//! viewport 的单元测试和 M0 的 golden 确定性回归。

use super::clock::{MAX_CLOCK_DELTA, clock_step};
use super::player::should_arm_fire_gate;

#[test]
fn fire_gate_should_ignore_the_launcher_mouse_hold() {
    assert!(!should_arm_fire_gate(false, true));
    assert!(!should_arm_fire_gate(true, true));
    assert!(should_arm_fire_gate(true, false));
}

#[test]
fn match_clock_should_not_finish_from_a_single_hitch() {
    let mut elapsed = 0.0;
    elapsed += clock_step(60.0);
    assert_eq!(elapsed, MAX_CLOCK_DELTA);
    assert!(elapsed < 60.0);
}

/// M0 的确定性回归：固定种子 + 固定输入脚本跑一个 headless App，
/// 命中/空枪/出生/消失事件序列的哈希必须逐位一致。
/// 重构时行为变了这里就会红；刻意改行为时重录 `GOLDEN_DIGEST`。
mod golden {
    use std::time::Duration;

    use bevy::input::mouse::MouseMotion;
    use bevy::prelude::*;
    use bevy::time::TimeUpdateStrategy;
    use bevy::window::PrimaryWindow;

    use crate::paths::AppPaths;
    use crate::sce::{LegacySpawn, ScenarioSpec};
    use crate::scenario::{PLAYER_EYE, TARGET_RADIUS, look_direction};
    use crate::settings::Settings;
    use crate::theme::Theme;

    use super::super::{
        AppPathsRes, FireGate, LiveTrace, Look, MatchClock, ScoreState, SettingsRes, SimPos,
        Target, ThemeRes, ViewportLaunch, ViewportMode, clock::tick_clock,
        combat::fire_hitscan, player::{apply_mouse_look, arm_fire_gate, update_camera},
        setup::setup,
    };

    const GOLDEN_DIGEST: u64 = 0xc20452198efc60e8;
    const GRID_DIGEST: u64 = 0xb47681a78160bb88;
    const FRAME_MS: u64 = 8;
    const DURATION_SECS: f32 = 4.0;
    const IDLE_CAP: u32 = 2_000;

    fn fnv1a64(bytes: &[u8]) -> u64 {
        let mut h = 0xcbf29ce484222325_u64;
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    fn wall_spec() -> ScenarioSpec {
        ScenarioSpec {
            live_count: 6,
            target_radius: TARGET_RADIUS,
            duration_secs: DURATION_SECS,
            legacy: Some(LegacySpawn::RandomWall),
            ..ScenarioSpec::fallback("GoldenWall")
        }
    }

    fn grid_spec() -> ScenarioSpec {
        ScenarioSpec {
            live_count: 3,
            target_radius: crate::scenario::GRID_RADIUS,
            duration_secs: DURATION_SECS,
            glossy: true,
            legacy: Some(LegacySpawn::Grid { cells: 9 }),
            ..ScenarioSpec::fallback("GoldenGrid")
        }
    }

    pub(super) fn build_app(root: std::path::PathBuf, scenario: ScenarioSpec) -> App {
        let settings = Settings {
            countdown_secs: 0,
            ..Settings::default()
        };
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                FRAME_MS,
            )))
            .add_message::<MouseMotion>()
            .insert_resource(ViewportLaunch {
                mode: ViewportMode::Play,
                scenario,
                session_id: "golden".into(),
                seed_override: Some(42),
                aimbot: false,
            })
            .insert_resource(AppPathsRes(AppPaths::from_root(root)))
            .insert_resource(SettingsRes(settings))
            .insert_resource(ThemeRes(Theme::fallback()))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<ColorMaterial>>()
            .insert_resource(ButtonInput::<MouseButton>::default())
            .init_state::<super::super::RunState>()
            .add_systems(Startup, setup)
            .add_systems(PreUpdate, (arm_fire_gate, apply_mouse_look).chain())
            .add_systems(Update, (update_camera, fire_hitscan, tick_clock).chain());
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        app
    }

    fn press_fire(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.update();
    }

    /// 当前活着的 id 最小的目标；返回把它放在准星正中所需的 yaw/pitch。
    fn aim_angles_at_lowest_id(app: &mut App) -> Option<(f32, f32)> {
        let world = app.world_mut();
        let mut query = world.query::<(&Target, &SimPos)>();
        let mut best: Option<(u32, Vec3)> = None;
        for (target, sim) in query.iter(world) {
            let pos = Vec3::new(sim.curr.x, sim.curr.y, sim.curr.z);
            if best.is_none_or(|(id, _)| target.id < id) {
                best = Some((target.id, pos));
            }
        }
        let (_, center) = best?;
        let d = center - Vec3::new(PLAYER_EYE.x, PLAYER_EYE.y, PLAYER_EYE.z);
        let len = d.length().max(1e-6);
        Some((
            d.x.atan2(-d.z).to_degrees(),
            (d.y / len).asin().to_degrees(),
        ))
    }

    fn aim_and_fire(app: &mut App) {
        let (yaw, pitch) = aim_angles_at_lowest_id(app).expect("live target");
        let mut look = app.world_mut().resource_mut::<Look>();
        look.yaw = yaw;
        look.pitch = pitch;
        press_fire(app);
    }

    fn miss_fire(app: &mut App) {
        let mut look = app.world_mut().resource_mut::<Look>();
        look.yaw = 0.0;
        look.pitch = 85.0;
        press_fire(app);
    }

    /// 哈希只取「语义值」：浮点用位模式，事件摊平成元组。
    /// 这样轨迹文件的字段改名 / 加列不会冲掉 golden，只有行为真的变了才会红。
    fn digest(app: &App) -> (u64, String) {
        let score = app.world().resource::<ScoreState>();
        let trace = &app.world().resource::<LiveTrace>().file;
        let mut bytes = Vec::new();
        let mut push = |v: u64| bytes.extend_from_slice(&v.to_le_bytes());
        push(score.hits as u64);
        push(score.misses as u64);
        for s in &trace.mouse {
            push(s.t_us);
            push(s.dx.to_bits() as u64);
            push(s.dy.to_bits() as u64);
            push(s.yaw_deg.to_bits() as u64);
            push(s.pitch_deg.to_bits() as u64);
        }
        for s in &trace.shots {
            push(s.t_us);
            push(s.hit as u64);
            push(u64::from(s.target_id));
            push(s.yaw_deg.to_bits() as u64);
            push(s.pitch_deg.to_bits() as u64);
        }
        for e in &trace.world {
            push(e.t_us);
            push(match e.kind {
                crate::session::WorldKind::Spawn => 0,
                crate::session::WorldKind::Despawn => 1,
                crate::session::WorldKind::Visible => 2,
                crate::session::WorldKind::Hidden => 3,
                crate::session::WorldKind::Killed => 4,
            });
            push(u64::from(e.id));
            push(e.pos.x.to_bits() as u64);
            push(e.pos.y.to_bits() as u64);
            push(e.pos.z.to_bits() as u64);
            push(e.radius.to_bits() as u64);
        }
        (
            fnv1a64(&bytes),
            serde_json::to_string_pretty(&serde_json::json!({
                "hits": score.hits,
                "misses": score.misses,
                "mouse": trace.mouse.len(),
                "shots": trace.shots,
                "world": trace.world,
            }))
            .unwrap(),
        )
    }

    /// 同一段脚本跑完一局：初始化 → 武装 → `hits` 次必中 + `misses` 次朝天
    /// → 空放到结束，返回事件哈希和详情。
    fn run_scripted(scenario: ScenarioSpec, hits: u32, misses: u32) -> (u64, String) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build_app(dir.path().to_path_buf(), scenario);

        // 第 1 帧：Startup 跑 setup + 两个鼠标位移采样。
        app.world_mut()
            .write_message(MouseMotion { delta: Vec2::new(30.0, -12.0) });
        app.world_mut()
            .write_message(MouseMotion { delta: Vec2::new(-8.0, 4.0) });
        app.update();
        // 让 FireGate 有时间武装（窗口聚焦 + 左键没按）。
        app.update();
        assert!(app.world().resource::<FireGate>().armed);

        for _ in 0..hits {
            aim_and_fire(&mut app);
        }
        for _ in 0..misses {
            miss_fire(&mut app);
        }
        // 验证脚本打的确实是准星方向：最后一次朝天枪的朝向直冲上。
        let look = app.world().resource::<Look>();
        let dir = look_direction(look.yaw, look.pitch);
        assert!(dir.y > 0.9);

        // 空放到自然结束。
        for _ in 0..IDLE_CAP {
            if app.world().resource::<MatchClock>().finished {
                break;
            }
            app.update();
        }
        assert!(app.world().resource::<MatchClock>().finished);
        digest(&app)
    }

    #[test]
    fn play_mode_should_match_golden_digest() {
        let (digest, detail) = run_scripted(wall_spec(), 6, 3);
        assert_eq!(
            digest, GOLDEN_DIGEST,
            "golden digest drifted, actual {digest:016x}\n{detail}"
        );
    }

    #[test]
    fn grid_mode_should_match_golden_digest() {
        // Gridshot：格子只有 3 个活的，打 3 中 3 空。
        let (digest, detail) = run_scripted(grid_spec(), 3, 3);
        assert_eq!(
            digest, GRID_DIGEST,
            "grid digest drifted, actual {digest:016x}\n{detail}"
        );
    }
}

/// M1 验收：脚本化完美跟枪——每帧把准星对到目标的插值位置
/// （玩家能做的极限），on-target 比例应接近 100%。
mod tracking {
    use std::time::Duration;

    use bevy::input::mouse::MouseMotion;
    use bevy::prelude::*;
    use bevy::time::TimeUpdateStrategy;
    use bevy::window::PrimaryWindow;

    use crate::paths::AppPaths;
    use crate::sce::{
        MotionSpec, ScenarioSpec, ScoringMode, SpawnGroup, SpawnRegion, SpawnRule, TargetShape,
    };
    use crate::scenario::PLAYER_EYE;
    use crate::settings::Settings;
    use crate::theme::Theme;

    use super::super::{
        AppPathsRes, Look, RenderPos, ScoreState, SettingsRes, Target, ThemeRes, ViewportLaunch,
        ViewportMode,
        clock::tick_clock,
        combat::{fire_hitscan, track_beam},
        director::{self, director_tick, sim_targets, sync_render_pos},
        player::{apply_mouse_look, arm_fire_gate, update_camera},
        setup::setup,
    };

    pub(super) fn tracking_spec() -> ScenarioSpec {
        let mut spec = ScenarioSpec::fallback("TrackTest");
        spec.legacy = None;
        spec.scoring.mode = ScoringMode::OnTargetPercent;
        spec.spawns = vec![SpawnGroup {
            hp: 0.0,
            headshot_only: false,
            bot: None,
            archetype: TargetShape::Sphere,
            radius: 0.35,
            no_shoot: false,
            count: 1,
            region: SpawnRegion::Box {
                min: crate::vec3::Vec3::new(-5.0, 1.2, -14.0),
                max: crate::vec3::Vec3::new(5.0, 2.4, -8.0),
            },
            motion: MotionSpec::Strafe {
                speed: (2.5, 3.5),
                interval: (0.8, 1.6),
                accel: 24.0,
                symmetric: true,
            },
            rule: SpawnRule::default(),
        }];
        spec
    }

    fn build_app(root: std::path::PathBuf) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(8)))
            .insert_resource(Time::<Fixed>::from_hz(director::SIM_HZ))
            .add_message::<MouseMotion>()
            .insert_resource(ViewportLaunch {
                mode: ViewportMode::Play,
                scenario: tracking_spec(),
                session_id: "tracking".into(),
                seed_override: Some(7),
                aimbot: false,
            })
            .insert_resource(AppPathsRes(AppPaths::from_root(root)))
            .insert_resource(SettingsRes(Settings {
                countdown_secs: 0,
                ..Settings::default()
            }))
            .insert_resource(ThemeRes(Theme::fallback()))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<ColorMaterial>>()
            .insert_resource(ButtonInput::<MouseButton>::default())
            .init_state::<super::super::RunState>()
            .add_systems(Startup, setup)
            // 测试里不卡 RunState::Playing，直接全量注册。
            .add_systems(FixedUpdate, sim_targets)
            .add_systems(PreUpdate, (arm_fire_gate, apply_mouse_look).chain())
            .add_systems(
                Update,
                (
                    update_camera,
                    sync_render_pos,
                    fire_hitscan,
                    track_beam,
                    director_tick,
                    tick_clock,
                )
                    .chain(),
            );
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        app
    }

    /// 把准星对准目标当前的插值位置。
    fn aim_at_target(app: &mut App) -> bool {
        let world = app.world_mut();
        let mut query = world.query::<(&Target, &RenderPos)>();
        let mut best: Option<(u32, Vec3)> = None;
        for (target, render) in query.iter(world) {
            let pos = Vec3::new(render.0.x, render.0.y, render.0.z);
            if best.is_none_or(|(id, _)| target.id < id) {
                best = Some((target.id, pos));
            }
        }
        let Some((_, center)) = best else {
            return false;
        };
        let d = center - Vec3::new(PLAYER_EYE.x, PLAYER_EYE.y, PLAYER_EYE.z);
        let len = d.length().max(1e-6);
        let mut look = world.resource_mut::<Look>();
        look.yaw = d.x.atan2(-d.z).to_degrees();
        look.pitch = (d.y / len).asin().to_degrees();
        true
    }

    #[test]
    fn smooth_strafe_script_should_stay_on_target() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build_app(dir.path().to_path_buf());
        app.update(); // Startup：setup + 首批目标
        app.update(); // FireGate 武装
        assert!(app.world().resource::<super::super::FireGate>().armed);

        let mut aimed_frames = 0u32;
        for _ in 0..240 {
            if aim_at_target(&mut app) {
                aimed_frames += 1;
            }
            app.update();
        }
        assert!(aimed_frames > 200, "目标应该一直在场 {aimed_frames}");
        let score = app.world().resource::<ScoreState>();
        assert!(score.total_ticks > 0, "tick 应该在跑");
        let rate = score.on_target_ticks as f64 / score.total_ticks as f64;
        assert!(
            rate > 0.95,
            "完美跟枪命中率应 >95%，实际 {:.1}% ({}/{})",
            rate * 100.0,
            score.on_target_ticks,
            score.total_ticks
        );
    }

    #[test]
    fn moving_targets_should_produce_keyframes() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build_app(dir.path().to_path_buf());
        for _ in 0..30 {
            app.update();
        }
        let trace = &app.world().resource::<super::super::LiveTrace>().file;
        assert!(
            trace.targets.len() >= 10,
            "240Hz 模拟应已写 60Hz 关键帧，实际 {}",
            trace.targets.len()
        );
        // 目标确实动了：首个和末个关键帧位置不同。
        let first = trace.targets.first().unwrap().pos;
        let last = trace.targets.last().unwrap().pos;
        assert!(
            first.distance(last) > 0.05,
            "目标应该移动过 first={first:?} last={last:?}"
        );
    }
}

/// M2 武器系统：连发 / 点射 / 弹匣换弹 / 后坐冲量。
mod weapons {
    use std::time::Duration;

    use bevy::input::mouse::MouseMotion;
    use bevy::prelude::*;
    use bevy::time::TimeUpdateStrategy;
    use bevy::window::PrimaryWindow;

    use crate::paths::AppPaths;
    use crate::sce::{ScoringMode, WeaponSpec};
    use crate::settings::Settings;
    use crate::theme::Theme;

    use super::super::{
        AppPathsRes, ScoreState, SettingsRes, ThemeRes, ViewportLaunch, ViewportMode,
        clock::tick_clock,
        combat::{WeaponState, advance_projectiles, fire_hitscan, track_beam, weapon_tick},
        director::{director_tick, sim_targets, sync_render_pos},
        player::{apply_mouse_look, arm_fire_gate, update_camera},
        setup::setup,
    };
    use super::tracking::tracking_spec;

    fn build(root: std::path::PathBuf, weapon: WeaponSpec) -> App {
        let mut spec = tracking_spec();
        spec.weapon = weapon;
        spec.scoring.mode = ScoringMode::LegacyHits;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(8)))
            .insert_resource(Time::<Fixed>::from_hz(super::super::director::SIM_HZ))
            .add_message::<MouseMotion>()
            .insert_resource(ViewportLaunch {
                mode: ViewportMode::Play,
                scenario: spec,
                session_id: "weapons".into(),
                seed_override: Some(7),
                aimbot: false,
            })
            .insert_resource(AppPathsRes(AppPaths::from_root(root)))
            .insert_resource(SettingsRes(Settings {
                countdown_secs: 0,
                ..Settings::default()
            }))
            .insert_resource(ThemeRes(Theme::fallback()))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<ColorMaterial>>()
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(ButtonInput::<KeyCode>::default())
            .init_state::<super::super::RunState>()
            .add_systems(Startup, setup)
            .add_systems(FixedUpdate, (sim_targets, advance_projectiles, weapon_tick))
            .add_systems(PreUpdate, (arm_fire_gate, apply_mouse_look).chain())
            .add_systems(
                Update,
                (
                    update_camera,
                    sync_render_pos,
                    fire_hitscan,
                    track_beam,
                    director_tick,
                    tick_clock,
                )
                    .chain(),
            );
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        app
    }

    fn hold(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
    }

    #[test]
    fn auto_rifle_should_fire_at_interval_and_drain_mag() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build(dir.path().to_path_buf(), WeaponSpec::rifle_auto());
        app.update();
        app.update();
        hold(&mut app);
        for _ in 0..120 {
            app.update();
        }
        let score = app.world().resource::<ScoreState>();
        // ~0.96s 模拟时间 / 0.0857s 间隔 ≈ 11 发。
        assert!(
            (9..=14).contains(&score.shots_fired),
            "连发间隔不对：{}",
            score.shots_fired
        );
        let w = app.world().resource::<WeaponState>();
        assert_eq!(w.mag_left, 30 - score.shots_fired as i32);
        assert!(w.acc_pitch > 0.0, "后坐应累积");
        assert!(w.punch_pitch > 0.0, "冲量应跟随");
    }

    #[test]
    fn burst_should_fire_exactly_three_per_click() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build(dir.path().to_path_buf(), WeaponSpec::rifle_burst());
        app.update();
        app.update();
        // 一次按下 → 3 发。
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        {
            // MinimalPlugins 没有 InputPlugin 自动清 just_*，手动模拟。
            let mut input = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            input.release(MouseButton::Left);
            input.clear();
        }
        for _ in 0..40 {
            app.update();
        }
        let score = app.world().resource::<ScoreState>();
        assert_eq!(score.shots_fired, 3, "点射应正好 3 发");
    }

    #[test]
    fn reload_should_block_fire_then_refill() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut w = WeaponSpec::rifle_auto();
        w.magazine = Some(3);
        w.reload_empty = 0.3;
        let mut app = build(dir.path().to_path_buf(), w);
        app.update();
        app.update();
        hold(&mut app);
        for _ in 0..40 {
            app.update();
        }
        let score = app.world().resource::<ScoreState>();
        assert_eq!(score.shots_fired, 3, "弹匣只有 3 发");
        {
            let ws = app.world().resource::<WeaponState>();
            assert!(ws.reload_end > 0.0, "空弹匣应触发换弹");
        }
        // 换弹 0.3s ≈ 38 帧，跑 80 帧保证完成 + 又能开火。
        for _ in 0..80 {
            app.update();
        }
        let score2 = app.world().resource::<ScoreState>();
        assert!(score2.shots_fired > 3, "换弹完成后应能继续开火");
    }
}

/// M4 玩家移动：WASD 前进 / 碰撞阻挡。
mod movement {
    use std::time::Duration;

    use bevy::input::mouse::MouseMotion;
    use bevy::prelude::*;
    use bevy::time::TimeUpdateStrategy;
    use bevy::window::PrimaryWindow;

    use crate::paths::AppPaths;
    use crate::sce::{Block, MovementPreset, PlayerSpec, ScoringMode};
    use crate::settings::Settings;
    use crate::theme::Theme;
    use crate::vec3::Vec3 as V3;

    use super::super::{
        AppPathsRes, PlayerBody, SettingsRes, ThemeRes, ViewportLaunch, ViewportMode,
        clock::tick_clock,
        combat::{advance_projectiles, bot_fire, fire_hitscan, track_beam, weapon_tick},
        director::{director_tick, sim_targets, sync_render_pos},
        player::{apply_mouse_look, arm_fire_gate, move_player, update_camera},
        setup::setup,
    };
    use super::tracking::tracking_spec;

    pub(super) fn build(root: std::path::PathBuf, blocks: Vec<Block>) -> App {
        let mut spec = tracking_spec();
        spec.player = PlayerSpec {
            movement_preset: MovementPreset::Tactical,
            ..Default::default()
        };
        spec.map.blocks = blocks;
        spec.scoring.mode = ScoringMode::LegacyHits;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(8)))
            .insert_resource(Time::<Fixed>::from_hz(super::super::director::SIM_HZ))
            .add_message::<MouseMotion>()
            .insert_resource(ViewportLaunch {
                mode: ViewportMode::Play,
                scenario: spec,
                session_id: "movement".into(),
                seed_override: Some(7),
                aimbot: false,
            })
            .insert_resource(AppPathsRes(AppPaths::from_root(root)))
            .insert_resource(SettingsRes(Settings {
                countdown_secs: 0,
                ..Settings::default()
            }))
            .insert_resource(ThemeRes(Theme::fallback()))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<ColorMaterial>>()
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(ButtonInput::<KeyCode>::default())
            .init_state::<super::super::RunState>()
            .add_systems(Startup, setup)
            .add_systems(
                FixedUpdate,
                (move_player, sim_targets, advance_projectiles, bot_fire, weapon_tick),
            )
            .add_systems(PreUpdate, (arm_fire_gate, apply_mouse_look).chain())
            .add_systems(
                Update,
                (
                    update_camera,
                    sync_render_pos,
                    fire_hitscan,
                    track_beam,
                    director_tick,
                    tick_clock,
                )
                    .chain(),
            );
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        app
    }

    #[test]
    fn wasd_should_move_and_stop() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build(dir.path().to_path_buf(), Vec::new());
        app.update();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        for _ in 0..120 {
            app.update();
        }
        {
            let body = app.world().resource::<PlayerBody>();
            assert!(body.pos.z < -1.0, "W 应前进 -z，实际 {}", body.pos.z);
            let hs = (body.vel.x * body.vel.x + body.vel.z * body.vel.z).sqrt();
            assert!(hs > 3.0, "应接近全速 {hs}");
        }
        {
            let mut k = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            k.release(KeyCode::KeyW);
            k.clear();
        }
        for _ in 0..120 {
            app.update();
        }
        let body = app.world().resource::<PlayerBody>();
        let hs = (body.vel.x * body.vel.x + body.vel.z * body.vel.z).sqrt();
        assert!(hs < 0.05, "松开 0.5s 应停 {hs}");
    }

    #[test]
    fn block_should_stop_the_player() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        // 玩家前方 z=-3..-2 一面墙。
        let mut app = build(
            dir.path().to_path_buf(),
            vec![Block {
                min: V3::new(-5.0, 0.0, -3.0),
                max: V3::new(5.0, 3.0, -2.0),
            }],
        );
        app.update();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        for _ in 0..240 {
            app.update();
        }
        let body = app.world().resource::<PlayerBody>();
        assert!(
            body.pos.z > -3.0 + 0.3,
            "应被墙挡住（含半径），实际 {}",
            body.pos.z
        );
    }
}

/// M6 AI：bot 到点开火掉玩家血。
mod bots {
    use crate::paths::AppPaths;
    use crate::sce::BotParams;

    use super::super::{LiveTrace, ViewportLaunch};
    use super::movement::build;

    #[test]
    fn bot_should_drain_player_hp() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build(dir.path().to_path_buf(), Vec::new());
        // 给场景加一个会还手的 bot 组（首个 update 前改 spec 才生效）。
        {
            let mut launch = app.world_mut().resource_mut::<ViewportLaunch>();
            launch.scenario.spawns[0].bot = Some(BotParams {
                aim_time: (0.01, 0.01),
                fire_interval: 0.05,
                accuracy: 1.0,
                damage: 5.0,
                strafe: false,
            });
        }
        app.update();
        app.update();
        for _ in 0..120 {
            app.update();
        }
        let php = app.world().resource::<super::super::combat::PlayerHp>();
        assert!(php.hp < php.max, "bot 应打掉血，实际 {}", php.hp);
        let trace = &app.world().resource::<LiveTrace>().file;
        assert!(!trace.damage_taken.is_empty(), "应有受击事件");
    }
}

/// M8 端到端：全部内置场景都能 headless 建 App 跑 60 帧不 panic。
mod e2e {
    use crate::paths::AppPaths;
    use crate::sce;

    use super::golden::build_app;

    #[test]
    fn every_bundled_scenario_should_boot_headless() {
        // 用仓库根的 res/scenarios（session 落在 repo cache/，同 dev 运行）。
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let paths = AppPaths::from_root(root.clone());
        paths.ensure_dirs().unwrap();
        let specs = sce::list_scenarios(&paths);
        assert!(!specs.is_empty());
        // 每个场景起 app 跑 60 帧；脚本不动也不开枪，纯验证 spawn 管线。
        for spec in specs {
            let mut app = build_app(root.clone(), spec.clone());
            for _ in 0..60 {
                app.update();
            }
        }
    }

    /// 端到端 playlist：PlanRun.advance 能按课表把下一关 spec 装好。
    #[test]
    fn playlist_advance_should_load_next_spec() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let paths = AppPaths::from_root(root);
        paths.ensure_dirs().unwrap();
        let specs = sce::list_scenarios(&paths);
        let db = crate::stats::StatsDb::open(&paths).unwrap();
        let playlist = crate::playlist::BUILTIN
            .iter()
            .find(|p| p.id == "voltaic-duel")
            .unwrap();
        let mut plan = crate::playlist::PlanRun::new(playlist);
        // 课表里每个场景 id 都得在盘上（防止改名断链）。
        for sid in playlist.scenarios {
            assert!(
                specs.iter().any(|s| s.id == *sid),
                "playlist 场景不存在: {sid}"
            );
        }
        assert!(plan.advance("sess-0", 100.0, Some(&db), &specs));
        assert!(plan.next_spec.is_some());
        assert!(plan.advance("sess-1", 90.0, Some(&db), &specs));
        assert!(!plan.advance("sess-2", 80.0, Some(&db), &specs));
        assert_eq!(plan.results.len(), 3);
    }
}

/// 自瞄端到端：AimbotCtl + aimbot_drive 走真实输入管线
/// （MouseMotion→apply_mouse_look→fire_hitscan），验证命中率与门禁。
mod aimbot_e2e {
    use std::time::Duration;

    use bevy::input::mouse::MouseMotion;
    use bevy::prelude::*;
    use bevy::time::TimeUpdateStrategy;
    use bevy::window::PrimaryWindow;

    use crate::aimbot::{AimbotCtl, aimbot_drive};
    use crate::paths::AppPaths;
    use crate::sce::{LegacySpawn, MotionSpec, ScenarioSpec, SpawnGroup, SpawnRegion, SpawnRule};
    use crate::scenario::{GRID_RADIUS, TARGET_RADIUS};
    use crate::settings::Settings;
    use crate::theme::Theme;

    use super::super::{
        AppPathsRes, FireGate, LiveTrace, ScoreState, SettingsRes, ThemeRes, ViewportLaunch,
        ViewportMode,
        clock::tick_clock,
        combat::{fire_hitscan, track_beam},
        director::{self, director_tick, sim_targets, sync_render_pos},
        player::{apply_mouse_look, arm_fire_gate, update_camera},
        setup::setup,
    };

    const FRAME_MS: u64 = 8;

    /// headless 没有 InputPlugin 的逐帧 `just_*` 清理——手动补上等价语义，
    /// 否则一次 `press()` 在按住期间每帧都是 `just_pressed`
    /// （真实 App 里 InputSystem 每 PreUpdate 清理，只真一帧）。
    fn clear_input_frame(mut b: ResMut<ButtonInput<MouseButton>>) {
        b.clear();
    }

    fn wall_spec() -> ScenarioSpec {
        ScenarioSpec {
            live_count: 6,
            target_radius: TARGET_RADIUS,
            duration_secs: 20.0,
            legacy: Some(LegacySpawn::RandomWall),
            ..ScenarioSpec::fallback("AimbotWall")
        }
    }

    fn grid_spec() -> ScenarioSpec {
        ScenarioSpec {
            live_count: 3,
            target_radius: GRID_RADIUS,
            duration_secs: 20.0,
            glossy: true,
            legacy: Some(LegacySpawn::Grid { cells: 9 }),
            ..ScenarioSpec::fallback("AimbotGrid")
        }
    }

    /// 慢速正弦靶（X 轴 ±2m，4s 周期）：验证非静止目标的追踪命中。
    fn sine_spec() -> ScenarioSpec {
        let mut spec = ScenarioSpec::fallback("AimbotSine");
        spec.duration_secs = 20.0;
        spec.legacy = None;
        spec.spawns = vec![SpawnGroup {
            hp: 1.0,
            headshot_only: false,
            bot: None,
            archetype: crate::sce::TargetShape::Sphere,
            radius: 0.3,
            no_shoot: false,
            count: 1,
            region: SpawnRegion::Box {
                min: crate::vec3::Vec3::new(-1.0, 1.5, -11.0),
                max: crate::vec3::Vec3::new(1.0, 1.5, -9.0),
            },
            motion: MotionSpec::Sine {
                amp: crate::vec3::Vec3::new(2.0, 0.0, 0.0),
                freq: crate::vec3::Vec3::new(0.25, 0.0, 0.0),
                phase: crate::vec3::Vec3::ZERO,
            },
            rule: SpawnRule::default(),
        }];
        spec
    }

    /// 静止人形靶 + headshot_only：只有爆头计分，打身体记空枪。
    /// 这是区分「瞄头 / 瞄中心」的最强信号。
    fn humanoid_spec() -> ScenarioSpec {
        let mut spec = ScenarioSpec::fallback("AimbotHumanoid");
        spec.duration_secs = 15.0;
        spec.legacy = None;
        spec.spawns = vec![SpawnGroup {
            hp: 1.0,
            headshot_only: true,
            bot: None,
            archetype: crate::sce::TargetShape::Humanoid,
            radius: 0.3,
            no_shoot: false,
            count: 1,
            region: SpawnRegion::Box {
                min: crate::vec3::Vec3::new(0.0, 0.9, -10.0),
                max: crate::vec3::Vec3::new(0.0, 0.9, -10.0),
            },
            motion: MotionSpec::Static,
            rule: SpawnRule::default(),
        }];
        spec
    }

    fn build_app(root: std::path::PathBuf, scenario: ScenarioSpec, aimbot: bool) -> App {
        build_app_cfg(
            root,
            scenario,
            aimbot,
            Settings {
                countdown_secs: 0,
                ..Settings::default()
            },
        )
    }

    fn build_app_cfg(
        root: std::path::PathBuf,
        scenario: ScenarioSpec,
        aimbot: bool,
        settings: Settings,
    ) -> App {
        build_app_cfg_fps(root, scenario, aimbot, settings, FRAME_MS)
    }

    /// `frame_ms`：模拟帧时长——8ms(125fps) 是理想环境，29ms(~34fps)
    /// 贴近真实低端环境（用户实测机），抖动问题只在粗帧率下暴露。
    fn build_app_cfg_fps(
        root: std::path::PathBuf,
        scenario: ScenarioSpec,
        aimbot: bool,
        settings: Settings,
        frame_ms: u64,
    ) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                frame_ms,
            )))
            .insert_resource(Time::<Fixed>::from_hz(director::SIM_HZ))
            .add_message::<MouseMotion>()
            .insert_resource(ViewportLaunch {
                mode: ViewportMode::Play,
                scenario,
                session_id: "aimbot".into(),
                seed_override: Some(7),
                aimbot,
            })
            .insert_resource(AimbotCtl::default())
            .insert_resource(AppPathsRes(AppPaths::from_root(root)))
            .insert_resource(SettingsRes(settings))
            .insert_resource(ThemeRes(Theme::fallback()))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<ColorMaterial>>()
            .insert_resource(ButtonInput::<MouseButton>::default())
            .init_state::<super::super::RunState>()
            .add_systems(Startup, setup)
            .add_systems(FixedUpdate, sim_targets)
            .add_systems(
                PreUpdate,
                (clear_input_frame, arm_fire_gate, apply_mouse_look).chain(),
            )
            .add_systems(
                Update,
                (
                    update_camera,
                    sync_render_pos,
                    aimbot_drive,
                    fire_hitscan,
                    track_beam,
                    director_tick,
                    tick_clock,
                )
                    .chain(),
            );
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        app
    }

    /// 跑 n 帧（可选按住激活键）；返回 (hits, misses, 鼠标采样条数)。
    fn run_frames(app: &mut App, n: u32, hold_aim_key: bool) -> (i64, i64, usize) {
        app.update(); // Startup + 首批目标
        app.update(); // FireGate 武装
        assert!(app.world().resource::<FireGate>().armed);
        if hold_aim_key {
            // 默认激活键是鼠标右键（Settings::default().aimbot_key）。
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Right);
        }
        for _ in 0..n {
            app.update();
        }
        let score = app.world().resource::<ScoreState>();
        let trace = &app.world().resource::<LiveTrace>().file;
        (score.hits, score.misses, trace.mouse.len())
    }

    #[test]
    fn aimbot_should_hit_wall_targets_via_real_input_path() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build_app(dir.path().to_path_buf(), wall_spec(), true);
        // 15 秒模拟：按住激活键，应多次命中，且轨迹证明走了 MouseMotion 管线。
        let (hits, misses, mouse_n) = run_frames(&mut app, (15_000 / FRAME_MS) as u32, true);
        assert!(mouse_n > 100, "MouseMotion 轨迹应为非空, got {mouse_n}");
        assert!(hits >= 10, "静态墙面 15s 应多次命中, hits={hits}");
        let total = hits + misses;
        assert!(
            total > 0 && hits * 10 >= total * 6,
            "命中率应 >=60%: {hits}/{total}"
        );
    }

    #[test]
    fn aimbot_should_hit_grid_targets() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build_app(dir.path().to_path_buf(), grid_spec(), true);
        let (hits, misses, _) = run_frames(&mut app, (15_000 / FRAME_MS) as u32, true);
        assert!(hits >= 8, "Gridshot 15s 应多次命中, hits={hits}");
        assert!(
            hits * 10 >= (hits + misses) * 5,
            "命中率应 >=50%: {hits}/{}",
            hits + misses
        );
    }

    #[test]
    fn aimbot_should_hit_slow_moving_target() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build_app(dir.path().to_path_buf(), sine_spec(), true);
        let (hits, _, _) = run_frames(&mut app, (20_000 / FRAME_MS) as u32, true);
        assert!(hits >= 2, "慢速移动靶 20s 应有命中, hits={hits}");
    }

    /// 34fps 诊断（用户实测机帧率）：整场跑满，打印命中数 + 抖动指标
    /// （单帧位移分位数、方向反转率）。`--nocapture` 看数字。
    /// 注意：headless 时间走 `TimeUpdateStrategy::ManualDuration`，
    /// `delta_secs` = 29ms 与真实 34fps 一致。
    #[test]
    fn aimbot_wall_at_34fps_should_report_jitter_metrics() {
        const MS: u64 = 29; // ~34fps
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build_app_cfg_fps(
            dir.path().to_path_buf(),
            wall_spec(),
            true,
            Settings {
                countdown_secs: 0,
                ..Settings::default()
            },
            MS,
        );
        app.update();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        for _ in 0..(15_000 / MS) {
            app.update();
        }
        let score = app.world().resource::<ScoreState>();
        let trace = &app.world().resource::<LiveTrace>().file;
        let mut mags: Vec<f64> = trace
            .mouse
            .iter()
            .map(|m| (f64::from(m.dx) * f64::from(m.dx) + f64::from(m.dy) * f64::from(m.dy)).sqrt())
            .collect();
        // 方向反转率：相邻非零帧 dx 变号比例（>15% = 来回拉扯感）。
        let mut flips = 0u32;
        let mut pairs = 0u32;
        for w in trace.mouse.windows(2) {
            let (a, b) = (w[0].dx, w[1].dx);
            if a.abs() > 0.5 && b.abs() > 0.5 {
                pairs += 1;
                if a.signum() != b.signum() {
                    flips += 1;
                }
            }
        }
        mags.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pct = |p: f64| {
            if mags.is_empty() {
                0.0
            } else {
                mags[((mags.len() - 1) as f64 * p) as usize]
            }
        };
        println!(
            "34fps diag: hits={} misses={} mouse_n={} |mag| p50={:.1} p90={:.1} p99={:.1} flips={}/{}",
            score.hits,
            score.misses,
            mags.len(),
            pct(0.5),
            pct(0.9),
            pct(0.99),
            flips,
            pairs
        );
        assert!(score.hits >= 8, "34fps 15s 应多次命中, hits={}", score.hits);
    }

    /// 匀速系三算法各跑一遍：AFG/NEXO/Nep 都应能收敛并命中。
    /// 低速场景里它们没有反应延迟，命中数应不少于 Rain 的量级。
    #[test]
    fn aimbot_const_algo_variants_should_hit() {
        use crate::settings::AimAlgo;
        for algo in [AimAlgo::Afg, AimAlgo::Nexo, AimAlgo::Nep] {
            let dir = tempfile::tempdir().unwrap();
            let paths = AppPaths::from_root(dir.path().to_path_buf());
            paths.ensure_dirs().unwrap();
            let mut app = build_app_cfg(
                dir.path().to_path_buf(),
                wall_spec(),
                true,
                Settings {
                    countdown_secs: 0,
                    aimbot_algo: algo,
                    ..Settings::default()
                },
            );
            let (hits, misses, mouse_n) = run_frames(&mut app, (15_000 / FRAME_MS) as u32, true);
            assert!(
                hits >= 8,
                "{algo:?} 静态墙面 15s 应多次命中, hits={hits} misses={misses}"
            );
            assert!(mouse_n > 100, "{algo:?} 应有输出轨迹, got {mouse_n}");
        }
    }

    /// 人形靶默认瞄头：`headshot_only` 下只有命中头部才计分；
    /// 同一配置改成瞄 Center，打身体全记空枪 → 命中应显著更少。
    #[test]
    fn aimbot_should_default_to_head_on_humanoid() {
        let run = |part: crate::settings::AimbotPart| {
            let dir = tempfile::tempdir().unwrap();
            let paths = AppPaths::from_root(dir.path().to_path_buf());
            paths.ensure_dirs().unwrap();
            let mut app = build_app_cfg(
                dir.path().to_path_buf(),
                humanoid_spec(),
                true,
                Settings {
                    countdown_secs: 0,
                    aimbot_part: part,
                    ..Settings::default()
                },
            );
            run_frames(&mut app, (12_000 / FRAME_MS) as u32, true)
        };
        let (head_hits, _, _) = run(crate::settings::AimbotPart::Head);
        assert!(
            head_hits >= 2,
            "默认瞄头在 headshot_only 场景应有命中, hits={head_hits}"
        );
        let (ctr_hits, _, _) = run(crate::settings::AimbotPart::Center);
        assert!(
            ctr_hits < head_hits,
            "瞄 Center 命中应显著少于瞄头: {ctr_hits} vs {head_hits}"
        );
    }

    /// 端到端整场：跑满 duration 到 `clock.finished`，统计命中率。
    /// 结果打印供汇报用（`--nocapture` 可见）。
    #[test]
    fn aimbot_full_match_should_finish_with_hit_stats() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build_app(dir.path().to_path_buf(), wall_spec(), true);
        app.update(); // Startup
        app.update(); // FireGate 武装
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right); // 按住激活键打完全场
        let mut frames = 0u32;
        while !app.world().resource::<super::super::MatchClock>().finished {
            app.update();
            frames += 1;
            assert!(frames < 60_000, "整场比赛应能在限定帧数内结束");
        }
        let score = app.world().resource::<ScoreState>();
        let total = score.hits + score.misses;
        let rate = if total > 0 {
            score.hits as f64 / total as f64
        } else {
            0.0
        };
        println!(
            "E2E full match: frames={frames} hits={} misses={} total={total} hit_rate={:.1}%",
            score.hits,
            score.misses,
            rate * 100.0
        );
        assert!(total > 0, "整场应有射击");
        assert!(rate >= 0.5, "整场命中率应 >=50%, got {:.1}%", rate * 100.0);
    }

    /// 持键语义：武装了但没按住激活键 → 不瞄不开火。
    #[test]
    fn aimbot_without_aim_key_should_do_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build_app(dir.path().to_path_buf(), wall_spec(), true);
        let (hits, misses, mouse_n) = run_frames(&mut app, (2_000 / FRAME_MS) as u32, false);
        assert_eq!(hits, 0);
        assert_eq!(misses, 0);
        assert!(mouse_n <= 1, "未持键不应产生输入, got {mouse_n}");
    }

    #[test]
    fn aimbot_disabled_should_do_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut app = build_app(dir.path().to_path_buf(), wall_spec(), false);
        // 武装为 false 时即使按住激活键也不应产生输入。
        let (hits, misses, mouse_n) = run_frames(&mut app, (2_000 / FRAME_MS) as u32, true);
        assert_eq!(hits, 0);
        assert_eq!(misses, 0);
        // setup.rs 会写 1 条初始朝向采样；自瞄关闭时不应再有新增。
        assert!(mouse_n <= 1, "关闭时不应产生输入, got {mouse_n}");
    }
}
