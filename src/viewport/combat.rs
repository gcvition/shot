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
use crate::sce::{FireMode, LegacySpawn, ScoringMode, ScoringSpec, WeaponKind};
use crate::scenario::{MIN_SEPARATION, next_grid_cell_n, sample_unique_point};
use crate::session::{HitPart, OnTargetSpan, ShotEvent, ShotHit, WorldEvent, WorldKind};
use crate::sfx::SfxEvent;
use crate::vec3::Vec3 as V3;

use super::targets::{Life, SpawnCtx, Team};
use super::{
    FireGate, Hitbox, Hitboxes, LiveTrace, Look, MatchClock, PlayerBody, RenderPos,
    SpawnDirector, Target, TargetMotion, TargetRng, ViewportLaunch, WORLD_LAYER,
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
    /// 跟枪判定用：准星在靶上的帧数 / 总帧数。
    pub on_target_ticks: u64,
    pub total_ticks: u64,
    /// 反应时间累计（毫秒）和样本数；只有目标的「第一次命中」计入。
    pub reaction_ms_sum: f64,
    pub reaction_count: u64,
    /// 当前 OnTargetSpan：准星正贴着哪个目标、从何时起（µs）。
    pub beam_on: Option<u32>,
    pub beam_since_us: u64,
    /// 被 AI 打死次数（M6）。
    #[allow(dead_code)]
    pub deaths: i64,
}

/// 结算分：按 `ScoringSpec` 折算 `ScoreState`。
/// `elapsed` = 实际打的时间（秒）。
pub(crate) fn final_score(score: &ScoreState, spec: &ScoringSpec, elapsed: f64) -> f64 {
    match spec.mode {
        ScoringMode::LegacyHits => score.hits as f64,
        ScoringMode::Kovaak => {
            let raw = spec.per_damage as f64 * score.damage
                + spec.per_kill as f64 * score.kills as f64
                + spec.per_time as f64 * elapsed
                - spec.loss_per_miss as f64 * score.misses as f64
                - spec.loss_per_damage_taken as f64 * 0.0
                - spec.loss_per_death as f64 * score.deaths as f64;
            let acc = if score.shots_fired > 0 {
                score.hits as f64 / score.shots_fired as f64
            } else {
                1.0
            };
            let raw = if spec.mult_accuracy { raw * acc } else { raw };
            if spec.mult_sqrt_acc {
                raw * acc.sqrt()
            } else {
                raw
            }
        }
        ScoringMode::OnTargetPercent => {
            if score.total_ticks == 0 {
                0.0
            } else {
                100.0 * score.on_target_ticks as f64 / score.total_ticks as f64
            }
        }
        ScoringMode::ReactionMs => {
            if score.reaction_count == 0 {
                0.0
            } else {
                // 反应越快分越高：1000ms → 0 分线性，0ms → 满分 100。
                (100.0 - score.reaction_ms_sum / score.reaction_count as f64 / 10.0)
                    .clamp(0.0, 100.0)
            }
        }
        ScoringMode::Custom => score.hits as f64,
    }
}

/// 音效线程的发送端。`SfxEvent::Hit` 命中、`Miss` 空枪，其余变体 M2 起用。
#[derive(Resource)]
pub(crate) struct AudioBank {
    pub tx: std::sync::mpsc::Sender<SfxEvent>,
}

/// 左键按下：从眼睛沿准星打一条射线，最近的 [`RenderPos`] 目标算命中。
/// legacy 目标照旧当场补一个；v2 目标交给 [`SpawnDirector`] 按组规则重生。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn fire_hitscan(
    mouse: Res<ButtonInput<MouseButton>>,
    gate: Res<FireGate>,
    look: Res<Look>,
    body: Res<PlayerBody>,
    mut score: ResMut<ScoreState>,
    mut targets: Query<(
        Entity,
        &mut Target,
        &RenderPos,
        &Hitboxes,
        Option<&mut Life>,
        Option<&mut TargetMotion>,
        Option<&mut TargetRng>,
    )>,
    mut spawn: SpawnCtx,
    mut director: ResMut<SpawnDirector>,
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
    for (entity, target, render, boxes, _, _, _) in targets.iter() {
        let center = render.0;
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
    // --- v2 目标：血量 / 阵营 / 受击反应 / 导演重生 ---
    let Ok((entity, mut target, _, _, life, motion, trng)) = targets.get_mut(entity) else {
        return;
    };
    if let Some(mut life) = life {
        if target.team == Team::Neutral {
            // Go/No-Go：打错目标——算空枪、扣分、目标消失、照样重生。
            score.shots_fired += 1;
            score.misses += 1;
            score.score -= launch.scenario.scoring.loss_per_miss as f64;
            play_sound(&audio, false);
            spawn.commands.entity(entity).despawn();
            trace.file.world.push(WorldEvent {
                t_us,
                kind: WorldKind::Despawn,
                id: hit.id,
                pos: V3::ZERO,
                radius: 0.0,
            });
            let (lo, hi) = director.groups[life.group as usize].rule.delay;
            director.schedule(life.group as usize, sample_delay(&mut spawn.rng.0, lo, hi));
            trace.file.shots.push(shot_hit_event(t_us, hit.id, &look, hit_center, 0.0));
            return;
        }
        score.hits += 1;
        score.shots_fired += 1;
        score.score = score.hits as f64;
        play_sound(&audio, true);
        // 受击反应（KovaaK DamageReaction：被打可能立刻变向）。
        if let (Some(mut m), Some(mut r)) = (motion, trng) {
            m.0.on_hit(director.sim_time, &mut r.0);
        }
        if !life.first_hit_done {
            life.first_hit_done = true;
            score.reaction_ms_sum +=
                (director.sim_time - life.spawned_at).max(0.0) as f64 * 1000.0;
            score.reaction_count += 1;
        }
        let dmg = if launch.scenario.weapon.damage > 0.0 {
            launch.scenario.weapon.damage
        } else {
            1.0
        };
        // 跟枪场景：点击只触发受击反应（ADAD 变向），不杀目标。
        let unkillable = matches!(
            launch.scenario.scoring.mode,
            ScoringMode::OnTargetPercent
        );
        if unkillable {
            trace.file.shots.push(shot_hit_event(t_us, hit.id, &look, hit_center, 0.0));
            return;
        }
        target.hp -= dmg;
        score.damage += dmg as f64;
        if target.hp > 0.0 {
            // 多血目标：只记伤害，不死不重生。
            trace.file.shots.push(shot_hit_event(t_us, hit.id, &look, hit_center, dmg));
            return;
        }
        score.kills += 1;
        spawn.commands.entity(entity).despawn();
        trace.file.world.push(WorldEvent {
            t_us,
            kind: WorldKind::Despawn,
            id: hit.id,
            pos: V3::ZERO,
            radius: 0.0,
        });
        let (lo, hi) = director.groups[life.group as usize].rule.delay;
        director.schedule(life.group as usize, sample_delay(&mut spawn.rng.0, lo, hi));
        trace.file.shots.push(shot_hit_event(t_us, hit.id, &look, hit_center, dmg));
        return;
    }

    // --- legacy 目标：原 v1 行为逐字保留 ---
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
                .filter(|(e, _, _, _, _, _, _)| *e != entity)
                .map(|(_, _, render, _, _, _, _)| render.0)
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
                .filter(|(e, _, _, _, _, _, _)| *e != entity)
                .map(|(_, target, _, _, _, _, _)| target.id as usize)
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
    trace.file.shots.push(shot_hit_event(t_us, hit.id, &look, hit_center, 0.0));
}

fn sample_delay(rng: &mut rand_chacha::ChaCha8Rng, lo: f32, hi: f32) -> f32 {
    use rand::Rng;
    if hi <= lo {
        lo
    } else {
        lo + rng.random::<f32>() * (hi - lo)
    }
}

fn shot_hit_event(t_us: u64, target_id: u32, look: &Look, pos: V3, damage: f32) -> ShotEvent {
    ShotEvent {
        t_us,
        hit: true,
        target_id,
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
            target_id,
            part: HitPart::Body as u8,
            damage,
            pos,
        }],
    }
}

/// 跟枪判定 + 光束武器。每帧在 `RenderPos` 上 ray test（不打模拟点，
/// 避免 tick/渲染不一致），命中记 `on_target_ticks` 并开 `OnTargetSpan`。
/// 触发条件：计分模式是 OnTargetPercent，或武器是 Beam / Auto。
#[allow(clippy::too_many_arguments)]
pub(crate) fn track_beam(
    mouse: Res<ButtonInput<MouseButton>>,
    gate: Res<FireGate>,
    look: Res<Look>,
    body: Res<PlayerBody>,
    mut score: ResMut<ScoreState>,
    targets: Query<(&Target, &RenderPos, &Hitboxes)>,
    mut trace: ResMut<LiveTrace>,
    time: Res<Time>,
    clock: Res<MatchClock>,
    launch: Res<ViewportLaunch>,
) {
    let spec = &launch.scenario;
    let tracking = matches!(spec.scoring.mode, ScoringMode::OnTargetPercent)
        || spec.weapon.kind == WeaponKind::Beam
        || spec.weapon.fire_mode == FireMode::Auto;
    let t_us = (time.elapsed_secs() * 1_000_000.0) as u64;
    let close_span = |score: &mut ScoreState, trace: &mut LiveTrace, t_us: u64| {
        if let Some(id) = score.beam_on.take() {
            trace.file.on_target.push(OnTargetSpan {
                start_us: score.beam_since_us,
                end_us: t_us,
                target_id: id,
            });
        }
    };
    if !tracking || clock.finished || clock.countdown > 0.0 || !gate.armed {
        close_span(&mut score, &mut trace, t_us);
        return;
    }
    let origin = body.eye();
    let dir = crate::scenario::look_direction(look.yaw, look.pitch);
    let mut best: Option<(u32, f32)> = None;
    for (target, render, boxes) in &targets {
        for hbox in boxes.0.iter() {
            let radius = match *hbox {
                Hitbox::Sphere { radius } => radius,
            };
            if let Some(t) = ray_sphere(origin, dir, render.0, radius)
                && best.is_none_or(|(_, bt)| t < bt)
            {
                best = Some((target.id, t));
            }
        }
    }
    match best {
        Some((id, _)) => {
            score.on_target_ticks += 1;
            if score.beam_on != Some(id) {
                close_span(&mut score, &mut trace, t_us);
                score.beam_on = Some(id);
                score.beam_since_us = t_us;
            }
            // Beam 武器：按住左键按 dps 累积伤害（击杀留给 M2 血量目标）。
            if spec.weapon.kind == WeaponKind::Beam
                && spec.weapon.damage > 0.0
                && mouse.pressed(MouseButton::Left)
            {
                score.damage +=
                    spec.weapon.damage as f64 * time.delta_secs() as f64;
            }
        }
        None => close_span(&mut score, &mut trace, t_us),
    }
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
