//! 玩家开火：判定、计分、补目标、写轨迹。
//!
//! [`ScoreState`] 由 `.sce` 的计分规则驱动（`ScoringSpec`）；legacy 场景
//! （`ScoringMode=LegacyHits`）就是「分 = 命中数」，和现在完全一样。
//!
//! `fire_hitscan` 一次 `just_pressed` 打一枪：从眼睛沿准星拉射线，
//! 取最近的 [`Hitboxes`] 交点。命中 → 计分 + 音效 + 立刻按场景规则补新目标。

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::hitscan::{Hit, ray_sphere};
use crate::sce::LegacySpawn;
use crate::scenario::{MIN_SEPARATION, next_grid_cell_n, sample_unique_point};
use crate::session::{HitPart, ShotEvent, ShotHit, WorldEvent, WorldKind};
use crate::sfx::SfxEvent;
use crate::vec3::Vec3 as V3;

use super::targets::SpawnCtx;
use super::{
    FireGate, Hitbox, Hitboxes, LiveTrace, Look, MatchClock, PlayerBody, SimPos, Target,
    ViewportLaunch, WORLD_LAYER,
};

/// 一局的计分。`score` 是展示分（legacy = hits）；其余字段喂指标和 HUD。
#[derive(Resource, Default)]
pub(crate) struct ScoreState {
    pub score: f64,
    pub hits: i64,
    pub misses: i64,
    pub damage: f64,
    pub kills: i64,
    pub headshots: i64,
    pub shots_fired: i64,
    /// 跟枪判定用（M1）：在靶 tick 数 / 总 tick 数。
    #[allow(dead_code)]
    pub on_target_ticks: u64,
    pub total_ticks: u64,
    /// 被 AI 打死次数（M6）。
    #[allow(dead_code)]
    pub deaths: i64,
}

/// 音效线程的发送端。`SfxEvent::Hit` 命中、`Miss` 空枪，其余变体 M2 起用。
#[derive(Resource)]
pub(crate) struct AudioBank {
    pub tx: std::sync::mpsc::Sender<SfxEvent>,
}

/// 左键按下：从眼睛沿准星打一条射线，最近的目标算命中，然后立刻补一个新目标。
#[allow(clippy::too_many_arguments)]
pub(crate) fn fire_hitscan(
    mouse: Res<ButtonInput<MouseButton>>,
    gate: Res<FireGate>,
    look: Res<Look>,
    body: Res<PlayerBody>,
    mut score: ResMut<ScoreState>,
    targets: Query<(Entity, &Target, &SimPos, &Hitboxes)>,
    mut spawn: SpawnCtx,
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
    let origin = body.eye();
    let dir = crate::scenario::look_direction(look.yaw, look.pitch);
    let mut hits = Vec::new();
    for (entity, target, sim, boxes) in &targets {
        let center = sim.curr;
        for hbox in boxes.0.iter() {
            let radius = match *hbox {
                Hitbox::Sphere { radius } => radius,
            };
            if let Some(t) = ray_sphere(origin, dir, center, radius) {
                hits.push((Hit { id: target.id, t }, entity, center, radius));
            }
        }
    }
    let Some((hit, entity, hit_center, _)) =
        hits.iter().copied().min_by(|a, b| a.0.t.total_cmp(&b.0.t))
    else {
        miss(&audio, &mut score, &mut trace, t_us, &look);
        return;
    };
    score.hits += 1;
    score.shots_fired += 1;
    score.score = score.hits as f64;
    play_sound(&audio, true);
    spawn.commands.entity(entity).despawn();
    trace.file.world.push(WorldEvent {
        t_us,
        kind: WorldKind::Despawn,
        id: hit.id,
        pos: V3::ZERO,
        radius: 0.0,
    });
    let (id, pos, radius) = match launch.scenario.legacy {
        Some(LegacySpawn::RandomWall) => {
            let remaining: Vec<V3> = targets
                .iter()
                .filter(|(e, _, _, _)| *e != entity)
                .map(|(_, _, sim, _)| sim.curr)
                .collect();
            let pos = sample_unique_point(
                &mut spawn.rng.0,
                &remaining,
                launch.scenario.target_radius,
                MIN_SEPARATION,
            );
            let id = spawn.next_id.0;
            spawn.next_id.0 += 1;
            (id, pos, launch.scenario.target_radius)
        }
        Some(LegacySpawn::Grid { cells }) => {
            let live_idx: Vec<usize> = targets
                .iter()
                .filter(|(e, _, _, _)| *e != entity)
                .map(|(_, target, _, _)| target.id as usize)
                .collect();
            let next = next_grid_cell_n(&mut spawn.rng.0, &live_idx, cells);
            (
                next as u32,
                super::targets::grid_pos(next, cells, launch.scenario.target_radius),
                launch.scenario.target_radius,
            )
        }
        None => unreachable!("legacy scenario required for hit-scan respawn"),
    };
    super::targets::spawn_sphere(
        &mut spawn.commands,
        &mut spawn.meshes,
        &mut spawn.cache,
        spawn.mats.normal.clone(),
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
        spray_index: 0,
        punch_yaw_deg: 0.0,
        punch_pitch_deg: 0.0,
        spread_x: 0.0,
        spread_y: 0.0,
        player_vel: V3::ZERO,
        ammo_after: -1,
        hits: vec![ShotHit {
            target_id: hit.id,
            part: HitPart::Body as u8,
            damage: 0.0,
            pos: hit_center,
        }],
    });
}

fn miss(audio: &AudioBank, score: &mut ScoreState, trace: &mut LiveTrace, t_us: u64, look: &Look) {
    score.misses += 1;
    score.shots_fired += 1;
    play_sound(audio, false);
    trace.file.shots.push(ShotEvent {
        t_us,
        hit: false,
        target_id: 0,
        yaw_deg: look.yaw,
        pitch_deg: look.pitch,
        spray_index: 0,
        punch_yaw_deg: 0.0,
        punch_pitch_deg: 0.0,
        spread_x: 0.0,
        spread_y: 0.0,
        player_vel: V3::ZERO,
        ammo_after: -1,
        hits: Vec::new(),
    });
}

fn play_sound(audio: &AudioBank, hit: bool) {
    let _ = audio.tx.send(if hit {
        SfxEvent::Hit
    } else {
        SfxEvent::Miss
    });
}
