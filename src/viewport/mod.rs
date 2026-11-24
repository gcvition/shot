//! Bevy 训练 / 回放窗口（进程里 **没有** GPUI）。
//!
//! # 给新手的心智模型
//!
//! Bevy 是 ECS：数据放在 **Resource**（全局只有一份）和 **Component**（挂在实体上），
//! 逻辑写在 **system**（普通函数，参数告诉引擎你要读什么）。
//!
//! 玩法循环按文件分：
//!
//! | 文件 | 装什么 |
//! |------|--------|
//! | [`setup`] | 开局一次：资源、三台相机、房间、第一批目标、准星、HUD |
//! | [`player`] | 玩家身体 [`PlayerBody`]、视角 [`Look`]、开火门 |
//! | [`targets`] | 目标 [`Target`] / [`SimPos`] / [`Hitboxes`]、生成与网格缓存 |
//! | [`combat`] | 开火判定 [`fire_hitscan`](combat::fire_hitscan)、计分 [`ScoreState`] |
//! | [`clock`] | 倒计时/计时 [`MatchClock`]、结束写盘、轨迹 [`LiveTrace`] |
//! | [`replay`] | 回放控制器和轨迹线 |
//! | [`hud`] | 倒计时 / 剩余时间文字 |
//! | [`window`] | 拉伸 RT、锁鼠标、Esc |
//! | [`gfx`] | 房间网格、材质、2D 网格工具 |
//! | [`state`] | [`RunState`]：局内状态机（回合 / 课程连播用） |
//!
//! # 为什么 3D 不直接画到窗口
//!
//! 训练分辨率（例如 1920×1080）和窗口大小可以不同（无边框会拉满显示器）。
//! 所以画面分三步：
//!
//! ```text
//! Camera3d (WORLD_LAYER) ──► 一张 Image 渲染目标（RT）
//! Camera2d  (HUD_LAYER)  ──► 同一张 RT（准星、倒计时叠在 3D 上面）
//! 第三个 Camera2d + Sprite ──► 把 RT 拉伸到整个窗口
//! ```
//!
//! **不要** 给 3D 相机加 `Mesh3d` 去改朝向：用 `Transform::look_at`。
//! 世界物体必须带 `RenderLayers::layer(WORLD_LAYER)`，否则 3D 相机看不见。
//!
//! # 一帧在干什么（Play 模式）
//!
//! 1. `PreUpdate`：[`PlaySet::Gate`] → [`PlaySet::Look`]（先武装开火，再转视角）
//! 2. `Update` 按 `PlaySet` 链序：
//!    `Camera` → `Fire` → `Clock` → `Replay` → `Hud` → `Present`
//!
//! 回放模式不注册 `Gate/Look/Fire`，改跑 [`replay`] 里的驱动系统，
//! `Camera/Hud/Present` 两种模式共用。
//!
//! # 几个容易踩的坑
//!
//! - **FireGate**：从菜单点进来时左键可能还按着。必须窗口聚焦 **且** 左键松开过，才允许开火。
//! - **倒计时**：`MatchClock.countdown > 0` 时可以转视角，但不能开枪，命中也不计时。
//! - **时钟跳跃**：`clock_step` 把单帧 dt 钳到 0.25s，避免卡顿一帧直接结束 60 秒。
//! - **Esc**：中途退出 **不计入** 成绩。只有倒计时后打满时长才 `finish_play`。
//! - **Sixshot** 是墙上 6 个很小的无光球体；**Gridshot** 是 3×3 格子里同时活 3 个有高光的球。
//!
//! 二次开发：加新 system 时注意 Query 别和已有 system 抢同一个 `ResMut`，
//! 冲突时 Bevy 启动会 panic。需要顺序就把它放进对应的 [`PlaySet`] 链。

mod clock;
mod combat;
pub(crate) mod director;
mod gfx;
mod hud;
mod player;
mod replay;
mod setup;
mod state;
mod targets;
mod window;

pub(crate) use clock::{LiveTrace, MatchClock};
pub(crate) use combat::{AudioBank, ScoreState};
pub(crate) use director::SpawnDirector;
pub(crate) use player::{DegPerCount, FireGate, Look, PlayerBody, ViewPunch};
pub(crate) use replay::ReplayCtl;
pub(crate) use state::RunState;
pub(crate) use targets::{
    Hitbox, Hitboxes, Kinematics, MeshCache, NextTargetId, RenderPos, RngRes, SimPos, Target,
    TargetMaterials, TargetMotion, TargetRng,
};

use bevy::audio::AudioPlugin;
use bevy::picking::PickingSettings;
use bevy::prelude::*;
use bevy::window::{
    EnabledButtons, MonitorSelection, WindowMode, WindowResolution,
};

use crate::paths::AppPaths;
use crate::sce::ScenarioSpec;
use crate::settings::{DisplayMode, Settings};
use crate::theme::Theme;

/// 3D 房间和目标所在的渲染层。HUD 相机故意看不到这一层。
pub(crate) const WORLD_LAYER: usize = 1;
/// 准星、倒计时、回放轨迹。画在同一张 RT 上，叠在 3D 之上。
pub(crate) const HUD_LAYER: usize = 2;

/// 启动参数。菜单进程通过命令行传进来，整个 App 期间不变。
#[derive(Resource, Clone)]
pub struct ViewportLaunch {
    pub mode: ViewportMode,
    pub scenario: ScenarioSpec,
    pub session_id: String,
    /// 测试固定种子用。None 时 Play 随机、Replay 读轨迹里的种子。
    pub seed_override: Option<u64>,
}

/// 训练还是回放。决定注册哪些 system、是否录鼠标。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewportMode {
    Play,
    Replay,
}

#[derive(Resource)]
pub(crate) struct AppPathsRes(pub(crate) AppPaths);

#[derive(Resource)]
pub(crate) struct SettingsRes(pub(crate) Settings);

#[derive(Resource)]
pub(crate) struct ThemeRes(pub(crate) Theme);

/// SystemSet：替代原来一条长 `.chain()`，顺序在这里一次定义清楚。
///
/// 规则：`Look` 在 `Camera` 前、`Camera` 在 `Fire` 前、`Fire` 在结算前。
/// 回放模式把 `Gate/Look/Fire` 整个换掉（见 [`run`] 的 `run_if`）。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PlaySet {
    /// PreUpdate：FireGate 武装（防「点开始」那一下误开枪）。
    Gate,
    /// PreUpdate：原始鼠标 → Look。
    Look,
    /// Update：眼睛位置 + Look → 相机 Transform。
    Camera,
    /// Update：玩家开火（hitscan 判定 + 补新目标 + 轨迹）+ 跟枪判定。
    Fire,
    /// Update：v2 出生导演（到期重生）。legacy 场景空转。
    Spawn,
    /// Update：计时、结束条件、状态同步。
    Clock,
    /// Update：回放驱动（只回放模式注册系统）。
    Replay,
    /// Update：HUD 文字。
    Hud,
    /// Update：拉伸 RT、锁鼠标、Esc。
    Present,
}

/// 启动 Bevy App。这个函数会阻塞到窗口关掉（或训练结束发 `AppExit`）。
pub fn run(mode: ViewportMode, scenario: ScenarioSpec, session_id: String) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    paths.ensure_dirs()?;
    let settings = Settings::load_or_default(&paths)?;
    let theme = Theme::load(&paths, &settings.theme_path).unwrap_or_else(|_| Theme::fallback());
    let window_mode = match settings.display_mode {
        DisplayMode::BorderlessStretch => {
            WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
        }
        DisplayMode::Windowed => WindowMode::Windowed,
    };
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Shot Play".into(),
                        mode: window_mode,
                        resolution: WindowResolution::new(
                            settings.render_width,
                            settings.render_height,
                        )
                        .with_scale_factor_override(1.0),
                        decorations: false,
                        resizable: false,
                        enabled_buttons: EnabledButtons {
                            minimize: false,
                            maximize: false,
                            close: false,
                        },
                        titlebar_show_buttons: false,
                        ..default()
                    }),
                    close_when_requested: false,
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: paths.root.display().to_string(),
                    ..default()
                })
                .disable::<AudioPlugin>(),
        )
        .insert_resource(PickingSettings {
            is_enabled: false,
            is_input_enabled: false,
            is_hover_enabled: false,
            is_window_picking_enabled: false,
            ..default()
        })
        .insert_resource(ViewportLaunch {
            mode,
            scenario,
            session_id,
            seed_override: None,
        })
        .insert_resource(AppPathsRes(paths))
        .insert_resource(SettingsRes(settings))
        .insert_resource(ThemeRes(theme))
        .init_state::<RunState>()
        // M1：目标运动跑 240Hz 固定步长；渲染端插值（见 director 模块注释）。
        .insert_resource(Time::<Fixed>::from_hz(director::SIM_HZ))
        // Startup 只跑一次：造房间、相机、第一批目标。
        .add_systems(Startup, setup::setup)
        // 先武装开火门，再采样鼠标；顺序反了会把「点开始」那一下当开枪。
        .configure_sets(
            PreUpdate,
            (PlaySet::Gate, PlaySet::Look).chain().run_if(is_play),
        )
        .add_systems(PreUpdate, player::arm_fire_gate.in_set(PlaySet::Gate))
        .add_systems(PreUpdate, player::apply_mouse_look.in_set(PlaySet::Look))
        // FixedUpdate：目标运动模拟。只在 Playing 跑（倒计时/结束冻结）。
        .add_systems(
            FixedUpdate,
            director::sim_targets
                .run_if(is_play)
                .run_if(in_state(RunState::Playing)),
        )
        .configure_sets(
            Update,
            (
                PlaySet::Camera,
                PlaySet::Fire,
                PlaySet::Spawn,
                PlaySet::Clock,
                PlaySet::Replay,
                PlaySet::Hud,
                PlaySet::Present,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                player::update_camera,
                director::sync_render_pos,
            )
                .in_set(PlaySet::Camera),
        )
        .add_systems(
            Update,
            (combat::fire_hitscan, combat::track_beam)
                .chain()
                .run_if(is_play)
                .in_set(PlaySet::Fire),
        )
        .add_systems(
            Update,
            director::director_tick
                .run_if(is_play)
                .in_set(PlaySet::Spawn),
        )
        .add_systems(
            Update,
            (
                clock::tick_clock.run_if(is_play),
                state::sync_run_state,
            )
                .chain()
                .in_set(PlaySet::Clock),
        )
        .add_systems(
            Update,
            (replay::replay_tick, replay::draw_trail)
                .chain()
                .run_if(is_replay)
                .in_set(PlaySet::Replay),
        )
        .add_systems(Update, hud::update_hud.in_set(PlaySet::Hud))
        .add_systems(
            Update,
            (
                window::stretch_present,
                window::grab_cursor,
                window::handle_escape,
            )
                .chain()
                .in_set(PlaySet::Present),
        )
        .run();
    Ok(())
}

pub(crate) fn is_play(launch: Res<ViewportLaunch>) -> bool {
    launch.mode == ViewportMode::Play
}

pub(crate) fn is_replay(launch: Res<ViewportLaunch>) -> bool {
    launch.mode == ViewportMode::Replay
}

#[cfg(test)]
mod tests;
