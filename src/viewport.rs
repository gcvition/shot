use bevy::audio::AudioPlugin;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::input::mouse::MouseMotion;
use bevy::picking::PickingSettings;
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;
use bevy::window::{
    CursorGrabMode, CursorOptions, EnabledButtons, MonitorSelection, PrimaryWindow, WindowMode,
    WindowResolution,
};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::fov::vertical_fov_radians;
use crate::hitscan::{closest_hit, ray_aabb, ray_sphere, Hit};
use crate::paths::AppPaths;
use crate::scenario::{
    grid_cell_aabb, grid_cell_center, initial_grid_live, initial_six_targets, look_direction,
    next_grid_cell, sample_unique_point, GRID_CELL, MIN_SEPARATION, PLAYER_EYE, TARGET_RADIUS,
    WALL_X, WALL_Y_MAX, WALL_Y_MIN, WALL_Z,
};
use crate::session::{
    MouseSample, SessionFile, SettingsSnapshot, ShotEvent, WorldEvent, WorldKind,
};
use crate::settings::{DisplayMode, ScenarioKind, Settings, SESSION_SECS};
use crate::stats::{SessionRecord, StatsDb};
use crate::theme::Theme;
use crate::vec3::Vec3 as V3;

const WORLD_LAYER: usize = 1;
const HUD_LAYER: usize = 2;
const MAX_CLOCK_DELTA: f32 = 0.25;

#[derive(Resource, Clone)]
pub struct ViewportLaunch {
    pub mode: ViewportMode,
    pub scenario: ScenarioKind,
    pub session_id: String,
}

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

#[derive(Resource)]
struct DegPerCount(f64);

#[derive(Resource)]
struct Look {
    yaw: f32,
    pitch: f32,
}

#[derive(Resource)]
struct MatchClock {
    elapsed: f32,
    duration: f32,
    finished: bool,
}

#[derive(Resource)]
struct ScoreBoard {
    hits: i64,
    misses: i64,
}

#[derive(Resource)]
struct LiveTrace {
    file: SessionFile,
}

#[derive(Resource)]
struct RngRes(ChaCha8Rng);

#[derive(Resource)]
struct NextTargetId(u32);

#[derive(Resource)]
struct AudioBank {
    tx: std::sync::mpsc::Sender<bool>,
}

#[derive(Resource, Default)]
struct FireGate {
    armed: bool,
}

#[derive(Resource)]
struct SphereMat(Handle<StandardMaterial>);

#[derive(Resource)]
struct ReplayCtl {
    file: SessionFile,
    time: f32,
    speed: f32,
    paused: bool,
}

#[derive(Resource)]
struct TrailBuf(Vec<Vec2>);

#[derive(Component)]
struct WorldCam;

#[derive(Component)]
struct HudCam;

#[derive(Component)]
struct PresentSprite;

#[derive(Component)]
struct HudText;

#[derive(Component)]
struct SphereTarget {
    id: u32,
    radius: f32,
}

#[derive(Component)]
struct GridCell {
    index: usize,
    live: bool,
}

#[derive(Component)]
struct LiveMat(Handle<StandardMaterial>);

#[derive(Component)]
struct DeadMat(Handle<StandardMaterial>);

pub fn run(mode: ViewportMode, scenario: ScenarioKind, session_id: String) -> anyhow::Result<()> {
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
                        title: "Shot".into(),
                        mode: window_mode,
                        resolution: WindowResolution::new(
                            settings.render_width,
                            settings.render_height,
                        ),
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
        .add_systems(Startup, setup)
        .add_systems(
            PreUpdate,
            apply_mouse_look.run_if(is_play),
        )
        .add_systems(
            Update,
            (
                arm_fire_gate.run_if(is_play),
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

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut color_materials: ResMut<Assets<ColorMaterial>>,
    asset_server: Res<AssetServer>,
    launch: Res<ViewportLaunch>,
    paths: Res<AppPathsRes>,
    settings: Res<SettingsRes>,
    theme: Res<ThemeRes>,
) {
    let settings = settings.0.clone();
    let deg = crate::sensitivity::deg_per_count(settings.cm_per_360, settings.dpi)
        .unwrap_or(0.06);
    commands.insert_resource(DegPerCount(deg));
    commands.insert_resource(Look {
        yaw: 0.0,
        pitch: 0.0,
    });
    commands.insert_resource(MatchClock {
        elapsed: 0.0,
        duration: SESSION_SECS,
        finished: false,
    });
    commands.insert_resource(ScoreBoard { hits: 0, misses: 0 });
    commands.insert_resource(TrailBuf(Vec::new()));
    commands.insert_resource(FireGate::default());
    let tx = match crate::wav::load_sfx_clips(&paths.0) {
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
    let wall = theme.0.wall_rgb();
    let floor = theme.0.floor_rgb();
    let ceil = theme.0.ceiling_rgb();
    let enemy = theme.0.enemy_rgb();

    let world_layer = RenderLayers::layer(WORLD_LAYER);
    let hud_layer = RenderLayers::layer(HUD_LAYER);

    let vfov = vertical_fov_radians(settings.fov_deg, settings.fov_kind, settings.render_aspect());
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
    ));

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
            intensity: 2_000_000.0,
            range: 40.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, 6.0, 2.0),
        RenderLayers::layer(WORLD_LAYER).with(0),
    ));

    let wall_mat = materials.add(pbr(wall, theme.0.wall_roughness.unwrap_or(0.4), theme.0.wall_metallic.unwrap_or(0.05)));
    let floor_mat = materials.add(pbr(floor, theme.0.floor_roughness.unwrap_or(0.8), 0.0));
    let ceil_mat = materials.add(pbr(ceil, 0.6, 0.05));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(WALL_X * 2.0 + 1.0, WALL_Y_MAX + 0.5, 0.2))),
        MeshMaterial3d(wall_mat),
        Transform::from_xyz(0.0, (WALL_Y_MAX + WALL_Y_MIN) * 0.5, WALL_Z - 0.15),
        world_layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(24.0, 0.2, 24.0))),
        MeshMaterial3d(floor_mat),
        Transform::from_xyz(0.0, 0.0, -4.0),
        world_layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(24.0, 0.2, 24.0))),
        MeshMaterial3d(ceil_mat),
        Transform::from_xyz(0.0, 5.2, -4.0),
        world_layer.clone(),
    ));

    let live_mat = materials.add(pbr(enemy, 0.2, 0.4));
    let dead_mat = materials.add(pbr([0.55, 0.55, 0.58], 0.8, 0.0));
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
    let snapshot = SettingsSnapshot::from_settings(&settings, settings.render_width, settings.render_height, 60);
    let mut trace = SessionFile::new(
        launch.session_id.clone(),
        launch.scenario,
        seed,
        snapshot,
    );

    match launch.scenario {
        ScenarioKind::SixTargets => {
            let poses = if launch.mode == ViewportMode::Play {
                initial_six_targets(&mut rng)
            } else {
                Vec::new()
            };
            let mut max_id = 0;
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
            commands.insert_resource(NextTargetId(max_id + 1));
        }
        ScenarioKind::GridShot => {
            let live = if launch.mode == ViewportMode::Play {
                initial_grid_live(&mut rng)
            } else {
                Vec::new()
            };
            for i in 0..9 {
                let c = grid_cell_center(i);
                let is_live = live.contains(&i);
                let mat = if is_live {
                    live_mat.clone()
                } else {
                    dead_mat.clone()
                };
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(GRID_CELL, GRID_CELL, 0.32))),
                    MeshMaterial3d(mat),
                    Transform::from_xyz(c.x, c.y, c.z),
                    GridCell {
                        index: i,
                        live: is_live,
                    },
                    LiveMat(live_mat.clone()),
                    DeadMat(dead_mat.clone()),
                    world_layer.clone(),
                ));
                if is_live {
                    trace.world.push(WorldEvent {
                        t_us: 0,
                        kind: WorldKind::Spawn,
                        id: i as u32,
                        pos: c,
                        radius: GRID_CELL * 0.5,
                    });
                }
            }
            commands.insert_resource(NextTargetId(9));
        }
    }
    commands.insert_resource(RngRes(rng));

    if launch.mode == ViewportMode::Replay {
        if let Ok(file) = SessionFile::load(&paths.0, &launch.session_id) {
            commands.insert_resource(MatchClock {
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
        commands.insert_resource(LiveTrace { file: trace });
    }

    let gap = 4.0;
    let arm = 12.0;
    let cross = color_materials.add(Color::srgb(0.05, 0.05, 0.05));
    for (w, h, x, y) in [
        (arm, 2.0, (gap + arm) * 0.5, 0.0),
        (arm, 2.0, -(gap + arm) * 0.5, 0.0),
        (2.0, arm, 0.0, (gap + arm) * 0.5),
        (2.0, arm, 0.0, -(gap + arm) * 0.5),
    ] {
        commands.spawn((
            Mesh2d(meshes.add(Rectangle::new(w, h))),
            MeshMaterial2d(cross.clone()),
            Transform::from_xyz(x, y, 1.0),
            hud_layer.clone(),
        ));
    }

    if let Some(image_path) = existing_res(&paths.0, &settings.crosshair_path) {
        let handle = asset_server.load::<Image>(rel_asset(&paths.0, &image_path));
        commands.spawn((
            Sprite {
                image: handle,
                custom_size: Some(Vec2::splat(28.0)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 2.0),
            hud_layer.clone(),
        ));
    }

    commands.spawn((
        Text2d::new("60.0  0"),
        TextFont {
            font_size: bevy::text::FontSize::Px(22.0),
            ..default()
        },
        TextColor(Color::srgb(0.08, 0.08, 0.1)),
        Transform::from_xyz(
            -(settings.render_width as f32) * 0.5 + 90.0,
            settings.render_height as f32 * 0.5 - 28.0,
            3.0,
        ),
        HudText,
        hud_layer,
    ));
}

fn pbr(rgb: [f32; 3], roughness: f32, metallic: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(rgb[0], rgb[1], rgb[2]),
        perceptual_roughness: roughness.clamp(0.04, 1.0),
        metallic: metallic.clamp(0.0, 1.0),
        ..default()
    }
}

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
        Mesh3d(meshes.add(Sphere::new(radius))),
        MeshMaterial3d(mat),
        Transform::from_xyz(pos.x, pos.y, pos.z),
        SphereTarget { id, radius },
        layer,
    ));
}

fn apply_mouse_look(
    mut motion: MessageReader<MouseMotion>,
    mut look: ResMut<Look>,
    deg: Res<DegPerCount>,
    time: Res<Time>,
    mut trace: ResMut<LiveTrace>,
    clock: Res<MatchClock>,
) {
    if clock.finished {
        return;
    }
    let t_us = (time.elapsed_secs() * 1_000_000.0) as u64;
    for event in motion.read() {
        let (yaw, pitch) =
            crate::sensitivity::apply_look(look.yaw, look.pitch, event.delta.x, event.delta.y, deg.0);
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

fn update_camera(look: Res<Look>, mut q: Query<&mut Transform, With<WorldCam>>) {
    let Ok(mut tf) = q.single_mut() else {
        return;
    };
    let dir = look_direction(look.yaw, look.pitch);
    let eye = Vec3::new(PLAYER_EYE.x, PLAYER_EYE.y, PLAYER_EYE.z);
    let target = eye + Vec3::new(dir.x, dir.y, dir.z);
    tf.look_at(target, Vec3::Y);
}

fn should_arm_fire_gate(focused: bool, left_held: bool) -> bool {
    focused && !left_held
}

fn clock_step(delta_secs: f32) -> f32 {
    delta_secs.clamp(0.0, MAX_CLOCK_DELTA)
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

fn fire_hitscan(
    mouse: Res<ButtonInput<MouseButton>>,
    gate: Res<FireGate>,
    look: Res<Look>,
    mut score: ResMut<ScoreBoard>,
    mut commands: Commands,
    spheres: Query<(Entity, &SphereTarget, &Transform)>,
    mut cells: Query<(
        Entity,
        &mut GridCell,
        &mut MeshMaterial3d<StandardMaterial>,
        &LiveMat,
        &DeadMat,
    )>,
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
    if clock.finished || !gate.armed || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let t_us = (time.elapsed_secs() * 1_000_000.0) as u64;
    let origin = V3::new(PLAYER_EYE.x, PLAYER_EYE.y, PLAYER_EYE.z);
    let dir = look_direction(look.yaw, look.pitch);
    let mut hits = Vec::new();
    match launch.scenario {
        ScenarioKind::SixTargets => {
            for (entity, target, tf) in &spheres {
                let center = V3::new(tf.translation.x, tf.translation.y, tf.translation.z);
                if let Some(t) = ray_sphere(origin, dir, center, target.radius) {
                    hits.push((Hit { id: target.id, t }, entity, center, target.radius));
                }
            }
            if let Some((hit, entity, _, _)) = hits
                .iter()
                .copied()
                .min_by(|a, b| a.0.t.total_cmp(&b.0.t))
            {
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
                let remaining: Vec<V3> = spheres
                    .iter()
                    .filter(|(e, _, _)| *e != entity)
                    .map(|(_, _, tf)| V3::new(tf.translation.x, tf.translation.y, tf.translation.z))
                    .collect();
                let pos = sample_unique_point(&mut rng.0, &remaining, TARGET_RADIUS, MIN_SEPARATION);
                let id = next_id.0;
                next_id.0 += 1;
                let mat = sphere_mat.0.clone();
                spawn_sphere(
                    &mut commands,
                    &mut meshes,
                    mat,
                    id,
                    pos,
                    TARGET_RADIUS,
                    RenderLayers::layer(WORLD_LAYER),
                );
                trace.file.world.push(WorldEvent {
                    t_us,
                    kind: WorldKind::Spawn,
                    id,
                    pos,
                    radius: TARGET_RADIUS,
                });
                trace.file.shots.push(ShotEvent {
                    t_us,
                    hit: true,
                    target_id: hit.id,
                    yaw_deg: look.yaw,
                    pitch_deg: look.pitch,
                });
            } else {
                miss(&audio, &mut score, &mut trace, t_us, &look);
            }
        }
        ScenarioKind::GridShot => {
            let mut grid_hits: Vec<(Hit, Entity)> = Vec::new();
            for (entity, cell, _, _, _) in &cells {
                if !cell.live {
                    continue;
                }
                let (min, max) = grid_cell_aabb(cell.index);
                if let Some(t) = ray_aabb(origin, dir, min, max) {
                    grid_hits.push((Hit { id: cell.index as u32, t }, entity));
                }
            }
            if let Some(best) = closest_hit(grid_hits.iter().map(|h| h.0)) {
                let Some((_, entity)) = grid_hits.iter().find(|h| h.0.id == best.id) else {
                    return;
                };
                let entity = *entity;
                score.hits += 1;
                play_sound(&audio, true);
                let mut live_idx = Vec::new();
                for (_, cell, _, _, _) in &cells {
                    if cell.live {
                        live_idx.push(cell.index);
                    }
                }
                live_idx.retain(|i| *i != best.id as usize);
                let next = next_grid_cell(&mut rng.0, &live_idx);
                for (e, mut cell, mut mat, live, dead) in &mut cells {
                    if e == entity {
                        cell.live = false;
                        mat.0 = dead.0.clone();
                        trace.file.world.push(WorldEvent {
                            t_us,
                            kind: WorldKind::Despawn,
                            id: cell.index as u32,
                            pos: V3::ZERO,
                            radius: 0.0,
                        });
                    }
                    if cell.index == next {
                        cell.live = true;
                        mat.0 = live.0.clone();
                        let c = grid_cell_center(next);
                        trace.file.world.push(WorldEvent {
                            t_us,
                            kind: WorldKind::Spawn,
                            id: next as u32,
                            pos: c,
                            radius: GRID_CELL * 0.5,
                        });
                    }
                }
                trace.file.shots.push(ShotEvent {
                    t_us,
                    hit: true,
                    target_id: best.id,
                    yaw_deg: look.yaw,
                    pitch_deg: look.pitch,
                });
            } else {
                miss(&audio, &mut score, &mut trace, t_us, &look);
            }
        }
    }
}

fn miss(
    audio: &AudioBank,
    score: &mut ScoreBoard,
    trace: &mut LiveTrace,
    t_us: u64,
    look: &Look,
) {
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
    clock.elapsed += clock_step(time.delta_secs());
    if clock.elapsed >= clock.duration {
        clock.finished = true;
        finish_play(&mut trace, &score, &launch, &paths.0, &settings.0);
        exit.write(AppExit::Success);
    }
}

fn finish_play(
    trace: &mut LiveTrace,
    score: &ScoreBoard,
    launch: &ViewportLaunch,
    paths: &AppPaths,
    settings: &Settings,
) {
    let duration_ms = (SESSION_SECS * 1000.0) as u32;
    trace.file.header.duration_ms = duration_ms;
    let _ = trace.file.save(paths);
    if let Ok(db) = StatsDb::open(paths) {
        let rec = SessionRecord {
            id: launch.session_id.clone(),
            scenario: launch.scenario,
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
            trace_path: paths.session_trace(&launch.session_id).display().to_string(),
            video_path: None,
            rng_seed: trace.file.header.rng_seed as i64,
        };
        let _ = db.insert(&rec);
    }
    let _ = crate::launch::write_last_session(paths, &launch.session_id);
    if settings.video_enabled {
        let _ = crate::launch::spawn_encode(&launch.session_id);
    }
}

fn replay_tick(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut replay: ResMut<ReplayCtl>,
    mut look: ResMut<Look>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    spheres: Query<Entity, With<SphereTarget>>,
    mut cells: Query<(
        &mut GridCell,
        &mut MeshMaterial3d<StandardMaterial>,
        &LiveMat,
        &DeadMat,
    )>,
    mut trail: ResMut<TrailBuf>,
    launch: Res<ViewportLaunch>,
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

    if launch.scenario == ScenarioKind::SixTargets {
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
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.05, 0.05),
            ..default()
        });
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
    } else {
        let mut live_set = std::collections::HashSet::new();
        for event in replay.file.world.iter().filter(|e| e.t_us <= t_us) {
            match event.kind {
                WorldKind::Spawn => {
                    live_set.insert(event.id);
                }
                WorldKind::Despawn => {
                    live_set.remove(&event.id);
                }
            }
        }
        for (mut cell, mut mat, live, dead) in &mut cells {
            let is_live = live_set.contains(&(cell.index as u32));
            cell.live = is_live;
            mat.0 = if is_live {
                live.0.clone()
            } else {
                dead.0.clone()
            };
        }
    }

    let dir = look_direction(yaw, pitch);
    if dir.z.abs() > 1e-4 {
        let t = (WALL_Z - PLAYER_EYE.z) / dir.z;
        if t > 0.0 {
            if let Some((sx, sy, _)) = crate::project::project_world(
                PLAYER_EYE.add(dir.scale(t)),
                PLAYER_EYE,
                yaw,
                pitch,
                1.2,
                16.0 / 9.0,
                400.0,
                300.0,
            ) {
                trail.0.push(Vec2::new(sx - 200.0, 150.0 - sy));
                if trail.0.len() > 80 {
                    trail.0.remove(0);
                }
            }
        }
    }
}

fn draw_trail(mut gizmos: Gizmos, trail: Res<TrailBuf>) {
    if trail.0.len() < 2 {
        return;
    }
    gizmos.linestrip_2d(trail.0.iter().copied(), Color::srgb(1.0, 0.35, 0.2));
}

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
            "{remain:04.1}  replay x{:.2}",
            replay.speed
        ));
    } else {
        let remain = (clock.duration - clock.elapsed).max(0.0);
        *text = Text2d::new(format!("{remain:04.1}  {}", score.hits));
    }
}

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

fn handle_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut clock: ResMut<MatchClock>,
    mut trace: Option<ResMut<LiveTrace>>,
    score: Res<ScoreBoard>,
    launch: Res<ViewportLaunch>,
    paths: Res<AppPathsRes>,
    settings: Res<SettingsRes>,
    mut exit: MessageWriter<AppExit>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if launch.mode == ViewportMode::Play && !clock.finished {
        clock.finished = true;
        if let Some(trace) = trace.as_mut() {
            finish_play(trace, &score, &launch, &paths.0, &settings.0);
        }
    }
    exit.write(AppExit::Success);
}

fn rel_asset(paths: &AppPaths, path: &std::path::Path) -> String {
    path.strip_prefix(&paths.root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

fn existing_res(paths: &AppPaths, relative: &str) -> Option<std::path::PathBuf> {
    let path = paths.resolve_res(relative);
    path.exists().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::{clock_step, should_arm_fire_gate, MAX_CLOCK_DELTA};
    use crate::settings::SESSION_SECS;

    #[test]
    fn fire_gate_should_ignore_the_launcher_mouse_hold() {
        assert!(!should_arm_fire_gate(false, true));
        assert!(!should_arm_fire_gate(true, true));
        assert!(should_arm_fire_gate(true, false));
    }

    #[test]
    fn match_clock_should_not_finish_from_a_single_hitch() {
        let mut elapsed = 0.0;
        elapsed += clock_step(SESSION_SECS);
        assert_eq!(elapsed, MAX_CLOCK_DELTA);
        assert!(elapsed < SESSION_SECS);
    }
}
