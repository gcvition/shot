//! Bevy 训练 / 回放窗口（进程里 **没有** GPUI）。
//!
//! # 给新手的心智模型
//!
//! Bevy 是 ECS：数据放在 **Resource**（全局只有一份）和 **Component**（挂在实体上），
//! 逻辑写在 **system**（普通函数，参数告诉引擎你要读什么）。
//!
//! 本文件就是整个玩法循环。想改「点了鼠标发生什么」，搜 `fire_hitscan`；
//! 想改「目标出现在哪」，看 `setup` 末尾和 [`crate::scenario`]。
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
//! 1. `PreUpdate`：`arm_fire_gate` → `apply_mouse_look`（先武装开火，再转视角）
//! 2. `Update` 按 `.chain()` 顺序：
//!    相机朝向 → 射线开火 → 走表 → HUD 文字 → 拉伸 RT → 锁鼠标 → Esc
//!
//! 回放模式关掉鼠标开火，改跑 `replay_tick` + `draw_trail`。
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
//! 冲突时 Bevy 启动会 panic。需要顺序就放进现有的 `.chain()`。

use bevy::asset::RenderAssetUsages;
use bevy::audio::AudioPlugin;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::MouseMotion;
use bevy::math::Affine2;
use bevy::mesh::Indices;
use bevy::mesh::PrimitiveTopology;
use bevy::picking::PickingSettings;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::{
    CursorGrabMode, CursorOptions, EnabledButtons, MonitorSelection, PrimaryWindow, WindowMode,
    WindowResolution,
};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::appearance::{self, TRAIL_WIDTH_PX, hud_from_screen, trail_screen_points};
use crate::fov::vertical_fov_radians;
use crate::hitscan::{Hit, ray_sphere};
use crate::paths::AppPaths;
use crate::sce::{ScenarioSpec, SpawnKind};
use crate::scenario::{
    MIN_SEPARATION, PLAYER_EYE, grid_cell_center_n, initial_grid_live_n, initial_random_targets,
    look_direction, next_grid_cell_n, sample_unique_point,
};
use crate::session::{
    MouseSample, SessionFile, SettingsSnapshot, ShotEvent, WorldEvent, WorldKind,
};
use crate::settings::{DisplayMode, Settings};
use crate::stats::{SessionRecord, StatsDb};
use crate::theme::Theme;
use crate::vec3::Vec3 as V3;

/// 3D 房间和目标所在的渲染层。HUD 相机故意看不到这一层。
const WORLD_LAYER: usize = 1;
/// 准星、倒计时、回放轨迹。画在同一张 RT 上，叠在 3D 之上。
const HUD_LAYER: usize = 2;
/// 单帧时钟最多走这么多秒。防止卡顿 1 秒就把 60 秒训练直接判完。
const MAX_CLOCK_DELTA: f32 = 0.25;

/// 启动参数。菜单进程通过命令行传进来，整个 App 期间不变。
#[derive(Resource, Clone)]
pub struct ViewportLaunch {
    pub mode: ViewportMode,
    pub scenario: ScenarioSpec,
    pub session_id: String,
}

/// 训练还是回放。决定注册哪些 system、是否录鼠标。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewportMode {
    Play,
    Replay,
}

#[derive(Resource)]
struct AppPathsRes(AppPaths);

#[derive(Resource)]
struct SettingsRes(Settings);

#[derive(Resource)]
struct ThemeRes(Theme);

/// 鼠标移动 1 个 count 时，视角转多少度。来自厘米/360 和 DPI。
#[derive(Resource)]
struct DegPerCount(f64);

/// 当前朝向。yaw 左右、pitch 上下，单位都是度。
#[derive(Resource)]
struct Look {
    yaw: f32,
    pitch: f32,
}

/// 一局的时间状态。
///
/// `countdown` 是开局倒计时（设置里 0..=30 秒）。倒计时没走完时
/// `elapsed` 不加，也不能开火。
#[derive(Resource)]
struct MatchClock {
    countdown: f32,
    elapsed: f32,
    duration: f32,
    finished: bool,
}

#[derive(Resource)]
struct ScoreBoard {
    hits: i64,
    misses: i64,
}

/// Play 模式边打边写的轨迹。结束时存成 `cache/sessions/<id>.shot`。
#[derive(Resource)]
struct LiveTrace {
    file: SessionFile,
}

#[derive(Resource)]
struct RngRes(ChaCha8Rng);

/// 下一个随机墙面目标的 id。格子模式用格子下标当 id，不走这个。
#[derive(Resource)]
struct NextTargetId(u32);

/// 命中音效线程的发送端。`true` = 命中，`false` = 空枪。
#[derive(Resource)]
struct AudioBank {
    tx: std::sync::mpsc::Sender<bool>,
}

/// 防止「从菜单点进来时左键还按着」立刻开一枪。
///
/// 必须窗口已聚焦 **并且** 当前没有按着左键，才把 `armed` 设为 true。
#[derive(Resource, Default)]
struct FireGate {
    armed: bool,
}

#[derive(Resource)]
struct SphereMat(Handle<StandardMaterial>);

/// 回放控制器。Space 暂停，1/2/3 调速，左右箭头跳一秒。
#[derive(Resource)]
struct ReplayCtl {
    file: SessionFile,
    time: f32,
    speed: f32,
    paused: bool,
}

/// 回放时准星扫过墙面的橙色轨迹网格。
#[derive(Component)]
struct TrailGfx;

/// 回放时命中/空枪在墙上的圆点。
#[derive(Component)]
struct HitMarkGfx;

/// 画 3D 世界的相机。每帧 `look_at` 准星方向。
#[derive(Component)]
struct WorldCam;

/// 把准星和文字画进 RT 的 2D 相机。
#[derive(Component)]
struct HudCam;

/// 把 RT 贴到窗口的那张全屏 Sprite。
#[derive(Component)]
struct PresentSprite;

/// 屏幕上方的倒计时 / 剩余时间文字。
#[derive(Component)]
struct HudText;

/// 活着的球体目标。命中判定用 `id` + `radius`，不读网格碰撞。
#[derive(Component)]
struct SphereTarget {
    id: u32,
    radius: f32,
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
        })
        .insert_resource(AppPathsRes(paths))
        .insert_resource(SettingsRes(settings))
        .insert_resource(ThemeRes(theme))
        // Startup 只跑一次：造房间、相机、第一批目标。
        .add_systems(Startup, setup)
        .add_systems(
            PreUpdate,
            // 先武装开火门，再采样鼠标；顺序反了会把「点开始」那一下当开枪。
            (arm_fire_gate, apply_mouse_look).chain().run_if(is_play),
        )
        .add_systems(
            Update,
            (
                update_camera,
                fire_hitscan.run_if(is_play),
                tick_clock.run_if(is_play),
                replay_tick.run_if(is_replay),
                draw_trail.run_if(is_replay),
                update_hud,
                stretch_present,
                grab_cursor,
                handle_escape,
            )
                .chain(),
        )
        .run();
    Ok(())
}

fn is_play(launch: Res<ViewportLaunch>) -> bool {
    launch.mode == ViewportMode::Play
}

fn is_replay(launch: Res<ViewportLaunch>) -> bool {
    launch.mode == ViewportMode::Replay
}

/// 建场景：资源、三台相机、房间、第一批球、准星、HUD。
///
/// 回放模式不在这里刷球，第一帧 `replay_tick` 会按 t=0 的 WorldEvent 生成。
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut color_materials: ResMut<Assets<ColorMaterial>>,
    launch: Res<ViewportLaunch>,
    paths: Res<AppPathsRes>,
    settings: Res<SettingsRes>,
    theme: Res<ThemeRes>,
) {
    let settings = settings.0.clone();
    let deg = crate::sensitivity::deg_per_count(settings.cm_per_360, settings.dpi).unwrap_or(0.06);
    commands.insert_resource(DegPerCount(deg));
    commands.insert_resource(Look {
        yaw: 0.0,
        pitch: 0.0,
    });
    commands.insert_resource(MatchClock {
        countdown: if launch.mode == ViewportMode::Play {
            settings.countdown_secs as f32
        } else {
            0.0
        },
        elapsed: 0.0,
        duration: launch.scenario.duration_secs,
        finished: false,
    });
    commands.insert_resource(ScoreBoard { hits: 0, misses: 0 });
    commands.insert_resource(FireGate::default());
    let tx = match crate::wav::load_sfx_clips(&paths.0, &settings.hit_sound, &settings.miss_sound) {
        Ok((hit, miss)) => crate::sfx::spawn_player(hit, miss, settings.master_volume),
        Err(_) => std::sync::mpsc::channel().0,
    };
    commands.insert_resource(AudioBank { tx });

    let mut image = Image::new_target_texture(
        settings.render_width,
        settings.render_height,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        None,
    );
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let rt = images.add(image);

    let sky = theme.0.sky_rgb();

    let world_layer = RenderLayers::layer(WORLD_LAYER);
    let hud_layer = RenderLayers::layer(HUD_LAYER);

    let vfov = vertical_fov_radians(
        settings.fov_deg,
        settings.fov_kind,
        settings.render_aspect(),
    );
    // 3D 相机画到 RT，不直接画窗口。order = -1 先于 HUD。
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::srgb(sky[0], sky[1], sky[2])),
            ..default()
        },
        RenderTarget::Image(rt.clone().into()),
        Projection::Perspective(PerspectiveProjection {
            fov: vfov,
            aspect_ratio: settings.render_aspect(),
            ..default()
        }),
        Transform::from_xyz(PLAYER_EYE.x, PLAYER_EYE.y, PLAYER_EYE.z),
        WorldCam,
        world_layer.clone(),
        AmbientLight {
            color: Color::srgb(1.0, 1.0, 1.0),
            brightness: 520.0,
            affects_lightmapped_meshes: true,
        },
    ));

    // HUD 相机也画进同一张 RT，清色设 None 以免把 3D 擦掉。
    commands.spawn((
        Camera2d,
        Camera {
            order: 0,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderTarget::Image(rt.clone().into()),
        HudCam,
        hud_layer.clone(),
    ));

    // 窗口相机：只负责把 RT Sprite 铺满屏幕。
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            ..default()
        },
    ));
    commands.spawn((
        Sprite {
            image: rt,
            custom_size: Some(Vec2::new(
                settings.render_width as f32,
                settings.render_height as f32,
            )),
            ..default()
        },
        PresentSprite,
    ));

    commands.spawn((
        PointLight {
            intensity: 14_000_000.0,
            range: 32.0,
            radius: 0.4,
            shadow_maps_enabled: false,
            color: Color::srgb(1.0, 1.0, 0.98),
            ..default()
        },
        Transform::from_xyz(0.4, 4.15, 0.3),
        RenderLayers::layer(WORLD_LAYER).with(0),
    ));
    commands.spawn((
        PointLight {
            intensity: 6_500_000.0,
            range: 24.0,
            radius: 0.25,
            shadow_maps_enabled: false,
            color: Color::srgb(0.95, 0.97, 1.0),
            ..default()
        },
        Transform::from_xyz(-1.6, 3.7, -2.8),
        RenderLayers::layer(WORLD_LAYER).with(0),
    ));

    spawn_kovaak_room(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        &theme.0,
        &launch.scenario,
        &world_layer,
    );

    let live_mat = materials.add(if launch.scenario.glossy {
        grid_target_mat(&theme.0)
    } else {
        six_target_mat()
    });
    commands.insert_resource(SphereMat(live_mat.clone()));

    let seed = if launch.mode == ViewportMode::Replay {
        match SessionFile::load(&paths.0, &launch.session_id) {
            Ok(file) => file.header.rng_seed,
            Err(_) => 1,
        }
    } else {
        rand::random::<u64>()
    };
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let snapshot = SettingsSnapshot::from_settings(
        &settings,
        settings.render_width,
        settings.render_height,
        60,
    );
    let mut trace = SessionFile::new(
        launch.session_id.clone(),
        launch.scenario.id.clone(),
        seed,
        snapshot,
    );

    let mut max_id = 0u32;
    if launch.mode == ViewportMode::Play {
        match launch.scenario.spawn {
            SpawnKind::RandomWall => {
                let poses = initial_random_targets(
                    &mut rng,
                    launch.scenario.live_count,
                    launch.scenario.target_radius,
                );
                for pose in poses {
                    spawn_sphere(
                        &mut commands,
                        &mut meshes,
                        live_mat.clone(),
                        pose.id,
                        pose.pos,
                        pose.radius,
                        world_layer.clone(),
                    );
                    trace.world.push(WorldEvent {
                        t_us: 0,
                        kind: WorldKind::Spawn,
                        id: pose.id,
                        pos: pose.pos,
                        radius: pose.radius,
                    });
                    max_id = max_id.max(pose.id);
                }
            }
            SpawnKind::Grid { cells } => {
                let live = initial_grid_live_n(&mut rng, launch.scenario.live_count, cells);
                for i in live {
                    let c = grid_pos(i, cells, launch.scenario.target_radius);
                    spawn_sphere(
                        &mut commands,
                        &mut meshes,
                        live_mat.clone(),
                        i as u32,
                        c,
                        launch.scenario.target_radius,
                        world_layer.clone(),
                    );
                    trace.world.push(WorldEvent {
                        t_us: 0,
                        kind: WorldKind::Spawn,
                        id: i as u32,
                        pos: c,
                        radius: launch.scenario.target_radius,
                    });
                    max_id = max_id.max(i as u32);
                }
            }
        }
    }
    commands.insert_resource(NextTargetId(max_id + 1));
    commands.insert_resource(RngRes(rng));

    if launch.mode == ViewportMode::Replay {
        if let Ok(file) = SessionFile::load(&paths.0, &launch.session_id) {
            commands.insert_resource(MatchClock {
                countdown: 0.0,
                elapsed: 0.0,
                duration: file.header.duration_ms as f32 / 1000.0,
                finished: false,
            });
            commands.insert_resource(ReplayCtl {
                file,
                time: 0.0,
                speed: 1.0,
                paused: false,
            });
        }
    } else {
        trace.mouse.push(MouseSample {
            t_us: 0,
            dx: 0.0,
            dy: 0.0,
            yaw_deg: 0.0,
            pitch_deg: 0.0,
        });
        commands.insert_resource(LiveTrace { file: trace });
    }

    let crosshair = appearance::load_crosshair(&paths.0, &settings.crosshair_path)
        .unwrap_or_else(appearance::fallback_crosshair);
    let cw = (crosshair.width as f32).max(6.0);
    let ch = (crosshair.height as f32).max(6.0);
    commands.spawn((
        Sprite {
            image: images.add(rgba_to_image(crosshair, true)),
            custom_size: Some(Vec2::new(cw, ch)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 2.0),
        hud_layer.clone(),
    ));

    let empty_trail = empty_mesh2d();
    let empty_marks = empty_mesh2d();
    commands.spawn((
        Mesh2d(meshes.add(empty_trail)),
        MeshMaterial2d(color_materials.add(Color::srgb(1.0, 0.38, 0.16))),
        Transform::from_xyz(0.0, 0.0, 0.6),
        Visibility::Hidden,
        TrailGfx,
        hud_layer.clone(),
    ));
    commands.spawn((
        Mesh2d(meshes.add(empty_marks)),
        MeshMaterial2d(color_materials.add(Color::srgb(0.35, 0.9, 0.4))),
        Transform::from_xyz(0.0, 0.0, 0.7),
        Visibility::Hidden,
        HitMarkGfx,
        hud_layer.clone(),
    ));

    commands.spawn((
        Text2d::new("0:60"),
        TextFont {
            font_size: bevy::text::FontSize::Px(36.0),
            ..default()
        },
        TextColor(Color::srgb(0.97, 0.97, 0.98)),
        Transform::from_xyz(0.0, settings.render_height as f32 * 0.5 - 46.0, 3.0),
        HudText,
        hud_layer.clone(),
    ));
    commands.spawn((
        Text2d::new(launch.scenario.display_name.clone()),
        TextFont {
            font_size: bevy::text::FontSize::Px(22.0),
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.95, 0.96)),
        Transform::from_xyz(0.0, -(settings.render_height as f32) * 0.5 + 38.0, 3.0),
        hud_layer,
    ));
}

fn six_target_mat() -> StandardMaterial {
    // Sixshot：很小的黑点，不吃光照，避免高光糊成一团。
    StandardMaterial {
        base_color: Color::srgb(0.02, 0.02, 0.022),
        unlit: true,
        ..default()
    }
}

fn grid_target_mat(theme: &Theme) -> StandardMaterial {
    // Gridshot：主题色金属球，粗糙度压低才会「亮」。
    let rgb = theme.enemy_rgb();
    StandardMaterial {
        base_color: Color::srgb(rgb[0].max(0.012), rgb[1].max(0.012), rgb[2].max(0.016)),
        perceptual_roughness: theme.enemy_roughness().clamp(0.06, 0.18),
        metallic: theme.enemy_metallic().max(0.78),
        reflectance: 0.62,
        ..default()
    }
}

fn unlit_tiled(
    texture: Handle<Image>,
    face_w: f32,
    face_h: f32,
    tile_m: f32,
    tint: Color,
) -> StandardMaterial {
    StandardMaterial {
        base_color: tint,
        base_color_texture: Some(texture),
        unlit: true,
        uv_transform: Affine2::from_scale(Vec2::new(
            (face_w / tile_m).max(0.5),
            (face_h / tile_m).max(0.5),
        )),
        ..default()
    }
}

/// 用主题色生成大理石墙 + 地砖，拼出一个封闭房间。目标贴在 `WALL_Z` 那面墙上。
fn spawn_kovaak_room(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    theme: &Theme,
    _scenario: &ScenarioSpec,
    layer: &RenderLayers,
) {
    let wall = theme.wall_rgb();
    let floor = theme.floor_rgb();
    let marble = images.add(rgba_to_image(appearance::marble_texture(wall, 512), false));
    let paver = images.add(rgba_to_image(appearance::brick_texture(floor, 512), false));

    let half = appearance::ROOM_HALF_X;
    let height = appearance::ROOM_HEIGHT;
    let thick = appearance::WALL_THICK;
    let front = appearance::room_front();
    let back = appearance::ROOM_BACK;
    let depth = (back - front).abs().max(0.5);
    let z_mid = (front + back) * 0.5;
    let tile = appearance::MARBLE_TILE_M;

    let floor_mat = materials.add(unlit_tiled(
        paver,
        half * 2.0,
        depth,
        appearance::PAVER_TILE_M,
        Color::WHITE,
    ));
    let wall_front = materials.add(unlit_tiled(
        marble.clone(),
        half * 2.0,
        height,
        tile,
        Color::WHITE,
    ));
    let wall_side = materials.add(unlit_tiled(
        marble.clone(),
        depth,
        height,
        tile,
        Color::WHITE,
    ));
    let ceil_mat = materials.add(unlit_tiled(
        marble.clone(),
        half * 2.0,
        depth,
        tile,
        Color::srgb(0.88, 0.88, 0.90),
    ));
    let ledge_mat = materials.add(unlit_tiled(marble, 2.2, 3.0, tile, Color::WHITE));

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(half * 2.0, thick, depth))),
        MeshMaterial3d(floor_mat),
        Transform::from_xyz(0.0, -thick * 0.5, z_mid),
        layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(half * 2.0, thick, depth))),
        MeshMaterial3d(ceil_mat),
        Transform::from_xyz(0.0, height + thick * 0.5, z_mid),
        layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(half * 2.0, height, thick))),
        MeshMaterial3d(wall_front.clone()),
        Transform::from_xyz(0.0, height * 0.5, front - thick * 0.5),
        layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(half * 2.0, height, thick))),
        MeshMaterial3d(wall_front),
        Transform::from_xyz(0.0, height * 0.5, back + thick * 0.5),
        layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(thick, height, depth))),
        MeshMaterial3d(wall_side.clone()),
        Transform::from_xyz(-half - thick * 0.5, height * 0.5, z_mid),
        layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(thick, height, depth))),
        MeshMaterial3d(wall_side),
        Transform::from_xyz(half + thick * 0.5, height * 0.5, z_mid),
        layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(2.2, 0.72, 3.0))),
        MeshMaterial3d(ledge_mat),
        Transform::from_xyz(-half + 1.1, 1.28, -2.4),
        layer.clone(),
    ));
}

fn rgba_to_image(src: appearance::RgbaImage, nearest: bool) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: src.width,
            height: src.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        src.pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = if nearest {
        ImageSampler::nearest()
    } else {
        ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            ..default()
        })
    };
    image
}

fn empty_mesh2d() -> Mesh {
    mesh2d_from_parts(
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
        vec![[0.0, 0.0], [0.0, 0.0], [0.0, 0.0]],
        vec![0, 1, 2],
    )
}

fn mesh2d_from_parts(positions: Vec<[f32; 3]>, uvs: Vec<[f32; 2]>, indices: Vec<u32>) -> Mesh {
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// 把一个球放进 WORLD_LAYER。`id` 给 hitscan 用，不要和别的活球重复。
fn spawn_sphere(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mat: Handle<StandardMaterial>,
    id: u32,
    pos: V3,
    radius: f32,
    layer: RenderLayers,
) {
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(radius).mesh().uv(
            if radius > 0.12 { 40 } else { 24 },
            if radius > 0.12 { 20 } else { 12 },
        ))),
        MeshMaterial3d(mat),
        Transform::from_xyz(pos.x, pos.y, pos.z),
        SphereTarget { id, radius },
        layer,
    ));
}

/// 每帧把鼠标位移累加到 yaw/pitch。倒计时期间也转视角，但不写入轨迹。
fn apply_mouse_look(
    mut motion: MessageReader<MouseMotion>,
    mut look: ResMut<Look>,
    deg: Res<DegPerCount>,
    time: Res<Time>,
    mut trace: ResMut<LiveTrace>,
    clock: Res<MatchClock>,
    gate: Res<FireGate>,
) {
    if clock.finished {
        for _ in motion.read() {}
        return;
    }
    if clock.countdown > 0.0 {
        for event in motion.read() {
            let (yaw, pitch) = crate::sensitivity::apply_look(
                look.yaw,
                look.pitch,
                event.delta.x,
                event.delta.y,
                deg.0,
            );
            look.yaw = yaw;
            look.pitch = pitch;
        }
        return;
    }
    if !gate.armed {
        for _ in motion.read() {}
        return;
    }
    let t_us = (time.elapsed_secs() * 1_000_000.0) as u64;
    for event in motion.read() {
        let (yaw, pitch) = crate::sensitivity::apply_look(
            look.yaw,
            look.pitch,
            event.delta.x,
            event.delta.y,
            deg.0,
        );
        look.yaw = yaw;
        look.pitch = pitch;
        trace.file.mouse.push(MouseSample {
            t_us,
            dx: event.delta.x,
            dy: event.delta.y,
            yaw_deg: yaw,
            pitch_deg: pitch,
        });
    }
}

/// 用 `Transform::look_at` 对准视线方向。不要改 Camera 的投影矩阵来「转头」。
fn update_camera(look: Res<Look>, mut q: Query<&mut Transform, With<WorldCam>>) {
    let Ok(mut tf) = q.single_mut() else {
        return;
    };
    let dir = look_direction(look.yaw, look.pitch);
    let eye = Vec3::new(PLAYER_EYE.x, PLAYER_EYE.y, PLAYER_EYE.z);
    let target = eye + Vec3::new(dir.x, dir.y, dir.z);
    tf.look_at(target, Vec3::Y);
}

/// 从菜单点进来时左键可能还按着：必须聚焦且左键已松开，才允许后续开火。
fn should_arm_fire_gate(focused: bool, left_held: bool) -> bool {
    focused && !left_held
}

/// 单帧 dt 钳在 0.25s，避免卡顿一帧把 60 秒直接判完。
fn clock_step(delta_secs: f32) -> f32 {
    delta_secs.clamp(0.0, MAX_CLOCK_DELTA)
}

fn grid_cols(cells: usize) -> usize {
    ((cells as f32).sqrt().round() as usize).max(1)
}

fn grid_pos(index: usize, cells: usize, radius: f32) -> V3 {
    let mut pos = grid_cell_center_n(index, grid_cols(cells));
    pos.z = crate::scenario::GRID_Z + radius;
    pos
}

fn arm_fire_gate(
    mut gate: ResMut<FireGate>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if gate.armed {
        return;
    }
    let focused = windows.iter().any(|window| window.focused);
    if should_arm_fire_gate(focused, mouse.pressed(MouseButton::Left)) {
        gate.armed = true;
    }
}

/// 左键按下：从眼睛沿准星打一条射线，最近的球算命中，然后立刻补一个新目标。
fn fire_hitscan(
    mouse: Res<ButtonInput<MouseButton>>,
    gate: Res<FireGate>,
    look: Res<Look>,
    mut score: ResMut<ScoreBoard>,
    mut commands: Commands,
    spheres: Query<(Entity, &SphereTarget, &Transform)>,
    mut rng: ResMut<RngRes>,
    mut next_id: ResMut<NextTargetId>,
    mut meshes: ResMut<Assets<Mesh>>,
    sphere_mat: Res<SphereMat>,
    audio: Res<AudioBank>,
    mut trace: ResMut<LiveTrace>,
    time: Res<Time>,
    clock: Res<MatchClock>,
    launch: Res<ViewportLaunch>,
) {
    if clock.finished
        || clock.countdown > 0.0
        || !gate.armed
        || !mouse.just_pressed(MouseButton::Left)
    {
        return;
    }
    let t_us = (time.elapsed_secs() * 1_000_000.0) as u64;
    let origin = V3::new(PLAYER_EYE.x, PLAYER_EYE.y, PLAYER_EYE.z);
    let dir = look_direction(look.yaw, look.pitch);
    let mut hits = Vec::new();
    for (entity, target, tf) in &spheres {
        let center = V3::new(tf.translation.x, tf.translation.y, tf.translation.z);
        if let Some(t) = ray_sphere(origin, dir, center, target.radius) {
            hits.push((Hit { id: target.id, t }, entity, center, target.radius));
        }
    }
    let Some((hit, entity, _, _)) = hits.iter().copied().min_by(|a, b| a.0.t.total_cmp(&b.0.t))
    else {
        miss(&audio, &mut score, &mut trace, t_us, &look);
        return;
    };
    score.hits += 1;
    play_sound(&audio, true);
    commands.entity(entity).despawn();
    trace.file.world.push(WorldEvent {
        t_us,
        kind: WorldKind::Despawn,
        id: hit.id,
        pos: V3::ZERO,
        radius: 0.0,
    });
    let (id, pos, radius) = match launch.scenario.spawn {
        SpawnKind::RandomWall => {
            let remaining: Vec<V3> = spheres
                .iter()
                .filter(|(e, _, _)| *e != entity)
                .map(|(_, _, tf)| V3::new(tf.translation.x, tf.translation.y, tf.translation.z))
                .collect();
            let pos = sample_unique_point(
                &mut rng.0,
                &remaining,
                launch.scenario.target_radius,
                MIN_SEPARATION,
            );
            let id = next_id.0;
            next_id.0 += 1;
            (id, pos, launch.scenario.target_radius)
        }
        SpawnKind::Grid { cells } => {
            let live_idx: Vec<usize> = spheres
                .iter()
                .filter(|(e, _, _)| *e != entity)
                .map(|(_, target, _)| target.id as usize)
                .collect();
            let next = next_grid_cell_n(&mut rng.0, &live_idx, cells);
            (
                next as u32,
                grid_pos(next, cells, launch.scenario.target_radius),
                launch.scenario.target_radius,
            )
        }
    };
    spawn_sphere(
        &mut commands,
        &mut meshes,
        sphere_mat.0.clone(),
        id,
        pos,
        radius,
        RenderLayers::layer(WORLD_LAYER),
    );
    trace.file.world.push(WorldEvent {
        t_us,
        kind: WorldKind::Spawn,
        id,
        pos,
        radius,
    });
    trace.file.shots.push(ShotEvent {
        t_us,
        hit: true,
        target_id: hit.id,
        yaw_deg: look.yaw,
        pitch_deg: look.pitch,
    });
}

fn miss(audio: &AudioBank, score: &mut ScoreBoard, trace: &mut LiveTrace, t_us: u64, look: &Look) {
    score.misses += 1;
    play_sound(audio, false);
    trace.file.shots.push(ShotEvent {
        t_us,
        hit: false,
        target_id: 0,
        yaw_deg: look.yaw,
        pitch_deg: look.pitch,
    });
}

fn play_sound(audio: &AudioBank, hit: bool) {
    let _ = audio.tx.send(hit);
}

/// 倒计时走完才开始 `elapsed`。到点后写 `.shot`、插入 SQLite、通知菜单读最后一局。
fn tick_clock(
    time: Res<Time>,
    mut clock: ResMut<MatchClock>,
    mut trace: ResMut<LiveTrace>,
    score: Res<ScoreBoard>,
    launch: Res<ViewportLaunch>,
    paths: Res<AppPathsRes>,
    settings: Res<SettingsRes>,
    mut exit: MessageWriter<AppExit>,
) {
    if clock.finished {
        return;
    }
    let dt = clock_step(time.delta_secs());
    if clock.countdown > 0.0 {
        clock.countdown = (clock.countdown - dt).max(0.0);
        return;
    }
    clock.elapsed += dt;
    if clock.elapsed >= clock.duration {
        clock.finished = true;
        finish_play(&mut trace, &score, &launch, &paths.0, &settings.0);
        exit.write(AppExit::Success);
    }
}

/// 打满时长才调用：写 `.shot`、插入成绩库、更新 last_session。
/// Esc 中途退出走 [`handle_escape`]，不要经过这里。
fn finish_play(
    trace: &mut LiveTrace,
    score: &ScoreBoard,
    launch: &ViewportLaunch,
    paths: &AppPaths,
    settings: &Settings,
) {
    let duration_ms = (launch.scenario.duration_secs * 1000.0) as u32;
    trace.file.header.duration_ms = duration_ms;
    let _ = trace.file.save(paths);
    if let Ok(db) = StatsDb::open(paths) {
        let rec = SessionRecord {
            id: launch.session_id.clone(),
            scenario: launch.scenario.id.clone(),
            started_at: trace.file.header.started_at_unix_ms,
            duration_ms,
            score: score.hits,
            hits: score.hits,
            misses: score.misses,
            cm_per_360: settings.cm_per_360,
            dpi: settings.dpi,
            fov_deg: settings.fov_deg,
            render_width: settings.render_width,
            render_height: settings.render_height,
            trace_path: paths
                .session_trace(&launch.session_id)
                .display()
                .to_string(),
            video_path: None,
            rng_seed: trace.file.header.rng_seed as i64,
        };
        let _ = db.insert(&rec);
    }
    let _ = crate::launch::write_last_session(paths, &launch.session_id);
}

/// 按当前回放时间重建活着的球，并覆盖 Look。每帧先清空再刷，实现简单。
fn replay_tick(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut replay: ResMut<ReplayCtl>,
    mut look: ResMut<Look>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    spheres: Query<Entity, With<SphereTarget>>,
    sphere_mat: Res<SphereMat>,
) {
    if keys.just_pressed(KeyCode::Space) {
        replay.paused = !replay.paused;
    }
    if keys.just_pressed(KeyCode::Digit1) {
        replay.speed = 0.25;
    }
    if keys.just_pressed(KeyCode::Digit2) {
        replay.speed = 1.0;
    }
    if keys.just_pressed(KeyCode::Digit3) {
        replay.speed = 2.0;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        replay.time = (replay.time - 1.0).max(0.0);
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        replay.time += 1.0;
    }
    if !replay.paused {
        replay.time += time.delta_secs() * replay.speed;
    }
    let duration = replay.file.header.duration_ms as f32 / 1000.0;
    replay.time = replay.time.clamp(0.0, duration.max(0.01));
    let t_us = (replay.time * 1_000_000.0) as u64;
    let (yaw, pitch) = replay.file.look_at(t_us);
    look.yaw = yaw;
    look.pitch = pitch;

    for entity in &spheres {
        commands.entity(entity).despawn();
    }
    let mut live = std::collections::HashMap::new();
    for event in replay.file.world.iter().filter(|e| e.t_us <= t_us) {
        match event.kind {
            WorldKind::Spawn => {
                live.insert(event.id, event);
            }
            WorldKind::Despawn => {
                live.remove(&event.id);
            }
        }
    }
    let mat = sphere_mat.0.clone();
    for event in live.values() {
        spawn_sphere(
            &mut commands,
            &mut meshes,
            mat.clone(),
            event.id,
            event.pos,
            event.radius,
            RenderLayers::layer(WORLD_LAYER),
        );
    }
}

/// 把最近约 0.55 秒的准星扫墙轨迹画成 2D 网格；命中点画小圆。
fn draw_trail(
    replay: Res<ReplayCtl>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut trail_q: Query<(&Mesh2d, &mut Visibility), (With<TrailGfx>, Without<HitMarkGfx>)>,
    mut mark_q: Query<(&Mesh2d, &mut Visibility), (With<HitMarkGfx>, Without<TrailGfx>)>,
) {
    let snap = &replay.file.header.settings;
    let render_w = snap.render_width.max(1) as f32;
    let render_h = snap.render_height.max(1) as f32;
    let aspect = render_w / render_h;
    let vfov = vertical_fov_radians(snap.fov_deg, snap.fov_kind, aspect);
    let t_us = (replay.time * 1_000_000.0) as u64;
    let wall_z = appearance::aim_wall_z();
    let (cam_yaw, cam_pitch) = replay.file.look_at(t_us);

    if let Ok((mesh2d, mut vis)) = trail_q.single_mut() {
        let screen = trail_screen_points(&replay.file, t_us, vfov, aspect, render_w, render_h);
        let hud: Vec<(f32, f32)> = screen
            .iter()
            .map(|&(sx, sy)| hud_from_screen(sx, sy, render_w, render_h))
            .collect();
        if hud.len() >= 2 {
            let (pos, uv, idx) = appearance::polyline_mesh(&hud, TRAIL_WIDTH_PX);
            if let Some(mut mesh) = meshes.get_mut(&mesh2d.0) {
                *mesh = mesh2d_from_parts(pos, uv, idx);
            }
            *vis = Visibility::Inherited;
        } else {
            *vis = Visibility::Hidden;
        }
    }

    if let Ok((mesh2d, mut vis)) = mark_q.single_mut() {
        let mut positions = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();
        for shot in replay
            .file
            .shots
            .iter()
            .filter(|s| s.t_us + 80_000 >= t_us && s.t_us <= t_us)
        {
            if let Some(hit) = appearance::wall_hit_at(shot.yaw_deg, shot.pitch_deg, wall_z) {
                if let Some((sx, sy, _)) = crate::project::project_world(
                    hit, PLAYER_EYE, cam_yaw, cam_pitch, vfov, aspect, render_w, render_h,
                ) {
                    let (hx, hy) = hud_from_screen(sx, sy, render_w, render_h);
                    let (p, u, idx) =
                        appearance::disk_mesh(hx, hy, if shot.hit { 6.0 } else { 4.5 });
                    let base = positions.len() as u32;
                    positions.extend(p);
                    uvs.extend(u);
                    indices.extend(idx.into_iter().map(|i| i + base));
                }
            }
        }
        if positions.is_empty() {
            *vis = Visibility::Hidden;
        } else if let Some(mut mesh) = meshes.get_mut(&mesh2d.0) {
            *mesh = mesh2d_from_parts(positions, uvs, indices);
            *vis = Visibility::Inherited;
        }
    }
}

/// 倒计时 / 剩余时间 / 回放倍速。只改带 `HudText` 的那一行。
fn update_hud(
    clock: Res<MatchClock>,
    score: Res<ScoreBoard>,
    replay: Option<Res<ReplayCtl>>,
    mut text: Query<&mut Text2d, With<HudText>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    if let Some(replay) = replay {
        let remain = (replay.file.header.duration_ms as f32 / 1000.0 - replay.time).max(0.0);
        *text = Text2d::new(format!(
            "{}:{:02}  replay x{:.2}",
            remain as u32 / 60,
            remain as u32 % 60,
            replay.speed
        ));
    } else if clock.countdown > 0.0 {
        *text = Text2d::new(format!("{}", clock.countdown.ceil() as u32));
    } else {
        let remain = (clock.duration - clock.elapsed).max(0.0);
        *text = Text2d::new(format!(
            "{}:{:02}   {}",
            remain as u32 / 60,
            remain as u32 % 60,
            score.hits
        ));
    }
}

/// 无边框时窗口可能比 RT 大：把 Present Sprite 拉成窗口大小（会拉伸，这是刻意的）。
fn stretch_present(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut sprite: Query<&mut Sprite, With<PresentSprite>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(mut sprite) = sprite.single_mut() else {
        return;
    };
    sprite.custom_size = Some(Vec2::new(window.width(), window.height()));
}

/// 训练时窗口聚焦就锁鼠标；回放要点一下左键才锁，方便拖动窗口。
fn grab_cursor(
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    launch: Res<ViewportLaunch>,
) {
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let want_grab = match launch.mode {
        ViewportMode::Play => window.focused,
        ViewportMode::Replay => {
            mouse.just_pressed(MouseButton::Left) || cursor.grab_mode != CursorGrabMode::None
        }
    };
    if !want_grab {
        return;
    }
    if cursor.visible {
        cursor.visible = false;
    }
    if cursor.grab_mode != CursorGrabMode::Confined {
        cursor.grab_mode = CursorGrabMode::Confined;
        let center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
        window.set_cursor_position(Some(center));
    }
}

/// Esc：立刻关窗口。没打满时长就不写 `.shot`、不进成绩库。
fn handle_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut clock: ResMut<MatchClock>,
    launch: Res<ViewportLaunch>,
    mut exit: MessageWriter<AppExit>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if launch.mode == ViewportMode::Play && !clock.finished {
        clock.finished = true;
    }
    exit.write(AppExit::Success);
}

#[cfg(test)]
mod tests {
    use super::{MAX_CLOCK_DELTA, clock_step, should_arm_fire_gate};

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
}
