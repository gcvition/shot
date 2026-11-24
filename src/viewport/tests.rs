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
