//! 开局一次：`setup` 造资源、三台相机、房间、第一批目标、准星、HUD。
//!
//! 回放模式不在这里刷目标，第一帧 `replay_tick` 会按 t=0 的 WorldEvent 生成。
//!
//! 这个 system 在 headless 测试里也跑（资源手动插进去就行，不碰窗口/GPU），
//! 所以新增资源记得在这里 insert，别只挂在渲染相关的分支里。

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::appearance;
use crate::fov::vertical_fov_radians;
use crate::sce::LegacySpawn;
use crate::scenario::{PLAYER_EYE, initial_grid_live_n, initial_random_targets};
use crate::session::{
    MouseSample, PlayerSample, SessionFile, SettingsSnapshot, WorldEvent, WorldKind, player_flags,
};
use crate::vec3::Vec3 as V3;

use super::{
    AppPathsRes, AudioBank, DegPerCount, FireGate, HUD_LAYER, LiveTrace, Look, MatchClock,
    MeshCache, NextTargetId, PlayerBody, ReplayCtl, RngRes, ScoreState, SettingsRes,
    TargetMaterials, ThemeRes, ViewPunch, ViewportLaunch, ViewportMode, WORLD_LAYER,
};

/// 把准星和文字画进 RT 的 2D 相机。
#[derive(Component)]
struct HudCam;

/// 把 RT 贴到窗口的那张全屏 Sprite。
#[derive(Component)]
pub(crate) struct PresentSprite;

/// 屏幕上方的倒计时 / 剩余时间文字。
#[derive(Component)]
pub(crate) struct HudText;

/// 回放时准星扫过墙面的橙色轨迹网格。
#[derive(Component)]
pub(crate) struct TrailGfx;

/// 回放时命中/空枪在墙上的圆点。
#[derive(Component)]
pub(crate) struct HitMarkGfx;

/// 建场景：资源、三台相机、房间、第一批球、准星、HUD。
#[allow(clippy::too_many_arguments)]
pub(crate) fn setup(
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
    commands.insert_resource(ViewPunch::default());
    commands.insert_resource(PlayerBody::default());
    commands.insert_resource(MatchClock {
        countdown: if launch.mode == ViewportMode::Play {
            settings.countdown_secs as f32
        } else {
            0.0
        },
        elapsed: 0.0,
        duration: launch.scenario.duration_secs,
        end: launch.scenario.end,
        finished: false,
    });
    commands.insert_resource(ScoreState::default());
    commands.insert_resource(FireGate::default());
    let tx = match crate::wav::load_sfx_clips(&paths.0, &settings.hit_sound, &settings.miss_sound) {
        Ok((hit, miss)) => crate::sfx::spawn_player(hit, miss, settings.master_volume),
        Err(_) => std::sync::mpsc::channel().0,
    };
    commands.insert_resource(AudioBank { tx });

    let mut image = Image::new_target_texture(
        settings.render_width,
        settings.render_height,
        TextureFormat::Rgba8UnormSrgb,
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
        super::player::WorldCam,
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

    super::gfx::spawn_kovaak_room(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        &theme.0,
        &world_layer,
    );

    let live_mat = materials.add(if launch.scenario.glossy {
        super::gfx::grid_target_mat(&theme.0)
    } else {
        super::gfx::six_target_mat()
    });
    commands.insert_resource(TargetMaterials {
        normal: live_mat.clone(),
        hit_flash: None,
        head: None,
    });
    let mut mesh_cache = MeshCache::default();

    let seed = if launch.mode == ViewportMode::Replay {
        match SessionFile::load(&paths.0, &launch.session_id) {
            Ok(file) => file.header.rng_seed,
            Err(_) => 1,
        }
    } else {
        launch.seed_override.unwrap_or_else(rand::random::<u64>)
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
        match launch.scenario.legacy {
            Some(LegacySpawn::RandomWall) => {
                let poses = initial_random_targets(
                    &mut rng,
                    launch.scenario.live_count,
                    launch.scenario.target_radius,
                );
                for pose in poses {
                    super::targets::spawn_sphere(
                        &mut commands,
                        &mut meshes,
                        &mut mesh_cache,
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
            Some(LegacySpawn::Grid { cells }) => {
                let live = initial_grid_live_n(&mut rng, launch.scenario.live_count, cells);
                for i in live {
                    let c = super::targets::grid_pos(i, cells, launch.scenario.target_radius);
                    super::targets::spawn_sphere(
                        &mut commands,
                        &mut meshes,
                        &mut mesh_cache,
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
            None => {}
        }
    }
    commands.insert_resource(mesh_cache);
    commands.insert_resource(NextTargetId(max_id + 1));
    commands.insert_resource(RngRes(rng));

    if launch.mode == ViewportMode::Replay {
        if let Ok(file) = SessionFile::load(&paths.0, &launch.session_id) {
            commands.insert_resource(MatchClock {
                countdown: 0.0,
                elapsed: 0.0,
                duration: file.header.duration_ms as f32 / 1000.0,
                end: launch.scenario.end,
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
        // 站桩场景只在开局写一条玩家状态；M4 起每个模拟 tick 写。
        trace.player.push(PlayerSample {
            t_us: 0,
            pos: V3::ZERO,
            vel: V3::ZERO,
            flags: player_flags::ON_GROUND,
            hp: 100.0,
        });
        commands.insert_resource(LiveTrace { file: trace });
    }

    let crosshair = appearance::load_crosshair(&paths.0, &settings.crosshair_path)
        .unwrap_or_else(appearance::fallback_crosshair);
    let cw = (crosshair.width as f32).max(6.0);
    let ch = (crosshair.height as f32).max(6.0);
    commands.spawn((
        Sprite {
            image: images.add(super::gfx::rgba_to_image(crosshair, true)),
            custom_size: Some(Vec2::new(cw, ch)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 2.0),
        hud_layer.clone(),
    ));

    let empty_trail = super::gfx::empty_mesh2d();
    let empty_marks = super::gfx::empty_mesh2d();
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
