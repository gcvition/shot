//! 回放：按 `.shot` 里的鼠标/世界事件重建现场。
//!
//! 轨迹是**权威数据**：回放不重跑模拟，只按 `t_us` 重放记录下来的状态。
//! 目前每帧清空所有目标再重建——目标会动之后（M5）改成按 id 的增量同步。

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::appearance::{self, TRAIL_WIDTH_PX, hud_from_screen, trail_screen_points};
use crate::fov::vertical_fov_radians;
use crate::session::{SessionFile, WorldKind};

use super::{
    Look, MeshCache, PlayerBody, Target, TargetMaterials, WORLD_LAYER,
    setup::{HitMarkGfx, TrailGfx},
};

/// 回放控制器。Space 暂停，1/2/3 调速，左右箭头跳一秒。
#[derive(Resource)]
pub(crate) struct ReplayCtl {
    pub file: SessionFile,
    pub time: f32,
    pub speed: f32,
    pub paused: bool,
}

/// 按当前回放时间重建活着的球，并覆盖 Look。每帧先清空再刷，实现简单。
#[allow(clippy::too_many_arguments)]
pub(crate) fn replay_tick(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut replay: ResMut<ReplayCtl>,
    mut look: ResMut<Look>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cache: ResMut<MeshCache>,
    targets: Query<Entity, With<Target>>,
    mats: Res<TargetMaterials>,
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

    for entity in &targets {
        commands.entity(entity).despawn();
    }
    let mut live = std::collections::HashMap::new();
    for event in replay.file.world.iter().filter(|e| e.t_us <= t_us) {
        match event.kind {
            WorldKind::Spawn | WorldKind::Visible => {
                live.insert(event.id, event);
            }
            WorldKind::Despawn | WorldKind::Hidden => {
                live.remove(&event.id);
            }
        }
    }
    let mat = mats.normal.clone();
    for event in live.values() {
        super::targets::spawn_sphere(
            &mut commands,
            &mut meshes,
            &mut cache,
            mat.clone(),
            event.id,
            event.pos,
            event.radius,
            RenderLayers::layer(WORLD_LAYER),
        );
    }
}

/// 把最近约 0.55 秒的准星扫墙轨迹画成 2D 网格；命中点画小圆。
#[allow(clippy::type_complexity)]
pub(crate) fn draw_trail(
    replay: Res<ReplayCtl>,
    body: Res<PlayerBody>,
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
                *mesh = super::gfx::mesh2d_from_parts(pos, uv, idx);
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
        let eye = body.eye();
        for shot in replay
            .file
            .shots
            .iter()
            .filter(|s| s.t_us + 80_000 >= t_us && s.t_us <= t_us)
        {
            if let Some(hit) = appearance::wall_hit_at(shot.yaw_deg, shot.pitch_deg, wall_z)
                && let Some((sx, sy, _)) = crate::project::project_world(
                    hit, eye, cam_yaw, cam_pitch, vfov, aspect, render_w, render_h,
                )
            {
                let (hx, hy) = hud_from_screen(sx, sy, render_w, render_h);
                let (p, u, idx) = appearance::disk_mesh(hx, hy, if shot.hit { 6.0 } else { 4.5 });
                let base = positions.len() as u32;
                positions.extend(p);
                uvs.extend(u);
                indices.extend(idx.into_iter().map(|i| i + base));
            }
        }
        if positions.is_empty() {
            *vis = Visibility::Hidden;
        } else if let Some(mut mesh) = meshes.get_mut(&mesh2d.0) {
            *mesh = super::gfx::mesh2d_from_parts(positions, uvs, indices);
            *vis = Visibility::Inherited;
        }
    }
}
