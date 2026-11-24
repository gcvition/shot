//! 玩家开火：判定、计分、补目标、写轨迹。
//!
//! [`ScoreState`] 由 `.sce` 的计分规则驱动（`ScoringSpec`）；legacy 场景
//! （`ScoringMode=LegacyHits`）就是「分 = 命中数」，和现在完全一样。
//!
//! `fire_hitscan` 一次 `just_pressed` 打一枪：从眼睛沿准星拉射线，
//! 取最近的 [`Hitboxes`] 交点。命中 → 计分 + 音效 + 立刻按场景规则补新目标。

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::hitscan::ray_sphere;
use crate::sce::{FireMode, LegacySpawn, ScoringMode, ScoringSpec, WeaponKind};
use crate::scenario::{MIN_SEPARATION, next_grid_cell_n, sample_unique_point};
use crate::session::{HitPart, OnTargetSpan, ShotEvent, ShotHit, WorldEvent, WorldKind};
use crate::sfx::SfxEvent;
use crate::vec3::Vec3 as V3;

use super::targets::{BotBrain, HeadOnly, Life, SpawnCtx, Team};
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
    pub deaths: i64,
    /// 累计受到的伤害（Kovaak 扣分项）。
    pub damage_taken: f64,
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
                - spec.loss_per_damage_taken as f64 * score.damage_taken
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

/// 开火状态（M2）：弹匣/换弹/点射余量/连发计时/累积后坐与视角冲量。
/// 计时一律用 director.sim_time（局内模拟秒），回放可复现。
#[derive(Resource, Default)]
pub(crate) struct WeaponState {
    /// 弹匣余量；<0 = 无弹匣武器。
    pub mag_left: i32,
    /// 正在换弹时的完成时刻（模拟秒）；0 = 不在换弹。
    pub reload_end: f64,
    /// 下次允许开火的模拟秒。
    pub next_fire: f64,
    /// Burst 组内剩余弹数。
    pub burst_left: u32,
    /// 累计射出弹数（后座表/轨迹用）。
    pub spray_index: u32,
    /// 扩散热度（度）。
    pub heat: f32,
    /// 累积后坐（弹道偏移，度）。
    pub acc_pitch: f32,
    pub acc_yaw: f32,
    /// 视角冲量 = acc * view_scale（只影响显示）。
    pub punch_pitch: f32,
    pub punch_yaw: f32,
}

impl WeaponState {
    /// 按当前武器初始化（setup 时调用）。
    pub(crate) fn for_weapon(spec: &crate::sce::WeaponSpec) -> Self {
        Self {
            mag_left: spec.magazine.map(|m| m as i32).unwrap_or(-1),
            ..Default::default()
        }
    }
}

/// 开火：按 fire_mode 判定扳机，处理弹匣/换弹/扩散/后坐，
/// hitscan 走射线判定，projectile 生成抛射物实体，beam 走 track_beam。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn fire_hitscan(
    mouse: Res<ButtonInput<MouseButton>>,
    gate: Res<FireGate>,
    look: Res<Look>,
    body: Res<PlayerBody>,
    mut score: ResMut<ScoreState>,
    mut wstate: ResMut<WeaponState>,
    mut targets: Query<(
        Entity,
        &mut Target,
        &RenderPos,
        &Hitboxes,
        Option<&mut Life>,
        Option<&mut TargetMotion>,
        Option<&mut TargetRng>,
        Option<&HeadOnly>,
    )>,
    mut spawn: SpawnCtx,
    mut director: ResMut<SpawnDirector>,
    audio: Res<AudioBank>,
    mut trace: ResMut<LiveTrace>,
    time: Res<Time>,
    clock: Res<MatchClock>,
    launch: Res<ViewportLaunch>,
) {
    if clock.finished || clock.countdown > 0.0 || !gate.armed {
        return;
    }
    let spec_w = &launch.scenario.weapon;
    if spec_w.kind == WeaponKind::Beam {
        return; // 光束伤害走 track_beam
    }
    let t = director.sim_time as f64;
    if wstate.reload_end > t {
        return; // 换弹中
    }
    let just = mouse.just_pressed(MouseButton::Left);
    let held = mouse.pressed(MouseButton::Left);
    // 弹匣空：这次按下开始换弹，不射出。
    if spec_w.magazine.is_some() && wstate.mag_left <= 0 {
        if just || held {
            wstate.reload_end = t + spec_w.reload_empty.max(0.05) as f64;
        }
        return;
    }
    let fire = match spec_w.fire_mode {
        FireMode::Semi => {
            // 半自动也守 RPM 上限（防止宏一帧一枪）。
            if just && t >= wstate.next_fire {
                wstate.next_fire = t + spec_w.fire_interval.max(0.01) as f64;
                true
            } else {
                false
            }
        }
        FireMode::Auto => {
            if held && t >= wstate.next_fire {
                wstate.next_fire = t + spec_w.fire_interval.max(0.01) as f64 as f64;
                true
            } else {
                false
            }
        }
        FireMode::Burst { count, interval } => {
            if just {
                wstate.burst_left = count;
            }
            if wstate.burst_left > 0 && t >= wstate.next_fire {
                wstate.burst_left -= 1;
                wstate.next_fire = t
                    + if wstate.burst_left > 0 {
                        interval.max(0.01) as f64
                    } else {
                        spec_w.fire_interval.max(0.05) as f64
                    };
                true
            } else {
                false
            }
        }
    };
    if !fire {
        return;
    }
    if let Some(_mag) = spec_w.magazine {
        wstate.mag_left -= 1;
        if wstate.mag_left <= 0 {
            wstate.reload_end = t + spec_w.reload_empty.max(0.05) as f64;
        }
    }
    wstate.spray_index += 1;
    let t_us = (time.elapsed_secs() * 1_000_000.0) as u64;
    let origin = body.eye();

    // 扩散热度（M4 起再叠速度惩罚）。
    let cap = if spec_w.spread.max > 0.0 {
        spec_w.spread.max
    } else {
        f32::MAX
    };
    // 移动惩罚（M4）：速度越快扩散越大。
    let move_pen = (body.vel.x * body.vel.x + body.vel.z * body.vel.z).sqrt() * 0.4;
    let spread_deg = (spec_w.spread.base + wstate.heat + move_pen).min(cap);
    wstate.heat = (wstate.heat + spec_w.spread.per_shot).min(cap);

    // 累积后坐偏「弹道」；视角冲量只是显示偏移。
    let aim_yaw = look.yaw + wstate.acc_yaw;
    let aim_pitch = (look.pitch + wstate.acc_pitch).clamp(-89.0, 89.0);
    let (rec_p, rec_y, rec_vs) = (spec_w.recoil.pitch, spec_w.recoil.yaw, spec_w.recoil.view_scale);
    if rec_p != 0.0 || rec_y != 0.0 {
        use rand::Rng;
        wstate.acc_pitch += rec_p;
        wstate.acc_yaw = if rec_y > 0.0 {
            spawn.rng.0.random_range(-rec_y..=rec_y)
        } else {
            0.0
        };
        wstate.punch_pitch = wstate.acc_pitch * rec_vs;
        wstate.punch_yaw = wstate.acc_yaw * rec_vs;
    }

    // 抛射物：本帧只生成实体，命中走 advance_projectiles。
    if let WeaponKind::Projectile {
        speed,
        gravity_scale,
        radius,
    } = spec_w.kind
    {
        use rand::Rng;
        let mesh = spawn.cache.sphere(&mut spawn.meshes, radius.max(0.05));
        for _ in 0..spec_w.pellets.max(1) {
            let (jy, jp) = if spread_deg > 0.0 {
                (
                    spawn.rng.0.random_range(-spread_deg..=spread_deg),
                    spawn.rng.0.random_range(-spread_deg..=spread_deg),
                )
            } else {
                (0.0, 0.0)
            };
            let dir = crate::scenario::look_direction(aim_yaw + jy, aim_pitch + jp);
            spawn.commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(spawn.mats.normal.clone()),
                Transform::from_xyz(origin.x, origin.y, origin.z),
                Projectile {
                    vel: dir.scale(speed),
                    grav: gravity_scale,
                    radius,
                    damage: spec_w.damage.max(1.0),
                    aoe: spec_w.aoe,
                    born: t,
                },
                RenderLayers::layer(WORLD_LAYER),
            ));
        }
        score.shots_fired += 1;
        // 不记 shot 事件：命中记 hit、到期记 miss，避免一弹两条。
        return;
    }

    // hitscan：每颗弹丸独立扩散抖动，取所有弹丸里最近的命中。
    let mut best: Option<(f32, Entity, V3, HitPart, (f32, f32))> = None;
    let mut last_jit = (0.0f32, 0.0f32);
    use rand::Rng;
    for _ in 0..spec_w.pellets.max(1) {
        let (jy, jp) = if spread_deg > 0.0 {
            (
                spawn.rng.0.random_range(-spread_deg..=spread_deg),
                spawn.rng.0.random_range(-spread_deg..=spread_deg),
            )
        } else {
            (0.0, 0.0)
        };
        last_jit = (jy, jp);
        let dir = crate::scenario::look_direction(
            aim_yaw + jy,
            (aim_pitch + jp).clamp(-89.0, 89.0),
        );
        // 子弹被墙挡：先求最近障碍 t，目标命中 t 更近才算。
        let block_t = nearest_block_t(origin, dir, &launch.scenario.map.blocks);
        for (entity, _target, render, boxes, _, _, _, _) in targets.iter() {
            for hbox in boxes.0.iter() {
                let hit = hitbox_ray(origin, dir, render.0, *hbox);
                if let Some((ht, part)) = hit
                    && ht < block_t
                    && best.is_none_or(|(bt, _, _, _, _)| ht < bt)
                {
                    best = Some((ht, entity, render.0, part, (jy, jp)));
                }
            }
        }
    }
    let Some((_, entity, hit_center, part, jit)) = best else {
        miss(&audio, &mut score, &mut trace, t_us, &look, &wstate, last_jit, body.vel);
        return;
    };
    let Ok((entity, mut target, _, boxes, life, motion, trng, head_only)) =
        targets.get_mut(entity)
    else {
        return;
    };
    // 爆头限定：身体命中按空枪算。没有头部件的目标（球体等）此键无效。
    let has_head = boxes
        .0
        .iter()
        .any(|h| matches!(h, Hitbox::OffsetSphere { .. }));
    if head_only.is_some() && has_head && part != HitPart::Head {
        miss(&audio, &mut score, &mut trace, t_us, &look, &wstate, jit, body.vel);
        return;
    }
    if part == HitPart::Head {
        score.headshots += 1;
    }
    let base_dmg = if spec_w.damage > 0.0 { spec_w.damage } else { 1.0 };
    let dmg = base_dmg
        * if part == HitPart::Head {
            spec_w.headshot_mult.max(1.0)
        } else {
            1.0
        };

    // --- v2 目标：血量 / 阵营 / 受击反应 / 导演重生 ---
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
                id: target.id,
                pos: V3::ZERO,
                radius: 0.0,
            });
            let (lo, hi) = director.groups[life.group as usize].rule.delay;
            director.schedule(life.group as usize, sample_delay(&mut spawn.rng.0, lo, hi));
            let mut ev = shot_hit_event(
                t_us,
                target.id,
                &look,
                hit_center,
                0.0,
                part,
                &wstate,
                jit,
                body.vel,
            );
            ev.hit = false; // 打错目标：计分是 miss，事件也要标 miss。
            trace.file.shots.push(ev);
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
        // 跟枪场景：点击只触发受击反应（ADAD 变向），不杀目标。
        let unkillable = matches!(
            launch.scenario.scoring.mode,
            ScoringMode::OnTargetPercent
        );
        if unkillable {
            trace.file.shots.push(shot_hit_event(
                t_us,
                target.id,
                &look,
                hit_center,
                0.0,
                part,
                &wstate,
                jit,
                body.vel,
            ));
            return;
        }
        target.hp -= dmg;
        score.damage += dmg as f64;
        if target.hp > 0.0 {
            trace.file.shots.push(shot_hit_event(
                t_us,
                target.id,
                &look,
                hit_center,
                dmg,
                part,
                &wstate,
                jit,
                body.vel,
            ));
            return;
        }
        score.kills += 1;
        spawn.commands.entity(entity).despawn();
        trace.file.world.push(WorldEvent {
            t_us,
            kind: WorldKind::Killed,
            id: target.id,
            pos: V3::ZERO,
            radius: 0.0,
        });
        let (lo, hi) = director.groups[life.group as usize].rule.delay;
        director.schedule(life.group as usize, sample_delay(&mut spawn.rng.0, lo, hi));
        trace.file.shots.push(shot_hit_event(
            t_us,
            target.id,
            &look,
            hit_center,
            dmg,
            part,
            &wstate,
            jit,
            body.vel,
        ));
        return;
    }

    // --- legacy 目标：原 v1 行为逐字保留 ---
    let hit_id = target.id;
    score.hits += 1;
    score.shots_fired += 1;
    score.score = score.hits as f64;
    play_sound(&audio, true);
    spawn.commands.entity(entity).despawn();
    trace.file.world.push(WorldEvent {
        t_us,
        kind: WorldKind::Despawn,
        id: target.id,
        pos: V3::ZERO,
        radius: 0.0,
    });
    let (id, pos, radius) = match launch.scenario.legacy {
        Some(LegacySpawn::RandomWall) => {
            let remaining: Vec<V3> = targets
                .iter()
                .filter(|(e, _, _, _, _, _, _, _)| *e != entity)
                .map(|(_, _, render, _, _, _, _, _)| render.0)
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
                .filter(|(e, _, _, _, _, _, _, _)| *e != entity)
                .map(|(_, target, _, _, _, _, _, _)| target.id as usize)
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
    trace.file.shots.push(shot_hit_event(
        t_us,
        hit_id,
        &look,
        hit_center,
        dmg,
        part,
        &wstate,
        last_jit,
        body.vel,
    ));
}

/// 命中体射线测试：返回 (t, 部位)。
pub(crate) fn hitbox_ray(
    origin: V3,
    dir: V3,
    center: V3,
    hbox: Hitbox,
) -> Option<(f32, HitPart)> {
    match hbox {
        Hitbox::Sphere { radius } => {
            ray_sphere(origin, dir, center, radius).map(|t| (t, HitPart::Body))
        }
        Hitbox::OffsetSphere { radius, offset } => {
            ray_sphere(origin, dir, center.add(offset), radius).map(|t| (t, HitPart::Head))
        }
        Hitbox::Capsule {
            radius,
            half_height,
            offset_y,
        } => crate::hitscan::ray_capsule(
            origin,
            dir,
            V3::new(center.x, center.y + offset_y, center.z),
            radius,
            half_height,
        )
        .map(|t| (t, HitPart::Body)),
    }
}

/// 抛射物实体（M2）。FixedUpdate 里推进。
#[derive(Component)]
pub(crate) struct Projectile {
    pub vel: V3,
    pub grav: f32,
    pub radius: f32,
    pub damage: f32,
    /// 爆炸半径（米）；0 = 单体弹（M8）。
    pub aoe: f32,
    pub born: f64,
}

/// 抛射物推进：匀速 + 重力，球扫掠判定（半径取 目标命中半径+弹半径）。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn advance_projectiles(
    mut projs: Query<(Entity, &mut Projectile, &mut Transform)>,
    mut targets: Query<(
        Entity,
        &mut Target,
        &RenderPos,
        &Hitboxes,
        Option<&mut Life>,
        Option<&mut TargetMotion>,
        Option<&mut TargetRng>,
        Option<&HeadOnly>,
    )>,
    mut spawn: SpawnCtx,
    mut director: ResMut<SpawnDirector>,
    mut score: ResMut<ScoreState>,
    audio: Res<AudioBank>,
    mut trace: ResMut<LiveTrace>,
    time: Res<Time>,
    clock: Res<MatchClock>,
    launch: Res<ViewportLaunch>,
    look: Res<Look>,
    wstate: Res<WeaponState>,
    body: Res<PlayerBody>,
) {
    if clock.finished || clock.countdown > 0.0 {
        return;
    }
    const DT: f32 = 1.0 / 240.0;
    let t = director.sim_time as f64;
    let t_us = (time.elapsed_secs() * 1_000_000.0) as u64;
    for (pe, mut p, mut tf) in projs.iter_mut() {
        p.vel.y -= 9.8 * p.grav * DT;
        let pos = V3::new(
            tf.translation.x + p.vel.x * DT,
            tf.translation.y + p.vel.y * DT,
            tf.translation.z + p.vel.z * DT,
        );
        // 地图遮挡：本 tick 的位移段被 block 截断 → 当场引爆。
        let step_len = pos.sub(V3::new(
            tf.translation.x,
            tf.translation.y,
            tf.translation.z,
        ))
        .length();
        let step_dir = pos
            .sub(V3::new(tf.translation.x, tf.translation.y, tf.translation.z))
            .normalized();
        let blocked = step_len > 1e-6
            && nearest_block_t(
                V3::new(tf.translation.x, tf.translation.y, tf.translation.z),
                step_dir,
                &launch.scenario.map.blocks,
            ) < step_len;
        tf.translation = Vec3::new(pos.x, pos.y, pos.z);
        if blocked {
            spawn.commands.entity(pe).despawn();
            continue;
        }
        // 宽体判定：弹心到目标命中区中心距离 < 目标半径+弹半径。
        let mut hit: Option<(f32, Entity, HitPart)> = None;
        for (te, _, render, boxes, _, _, _, _) in targets.iter() {
            for hbox in boxes.0.iter() {
                let (c, r, part) = match *hbox {
                    Hitbox::Sphere { radius } => (render.0, radius, HitPart::Body),
                    Hitbox::OffsetSphere { radius, offset } => {
                        (render.0.add(offset), radius, HitPart::Head)
                    }
                    Hitbox::Capsule {
                        radius,
                        half_height,
                        offset_y,
                    } => {
                        // 胶囊：点到轴线的最近点距离近似（抛射物精度足够）。
                        let axis = V3::new(render.0.x, render.0.y + offset_y, render.0.z);
                        let dy = (pos.y - axis.y).clamp(-half_height, half_height);
                        let closest = V3::new(axis.x, axis.y + dy, axis.z);
                        if pos.sub(closest).length() < radius + p.radius {
                            let d = pos.sub(render.0).length();
                            if hit.is_none_or(|(hd, _, _)| d < hd) {
                                hit = Some((d, te, HitPart::Body));
                            }
                        }
                        continue;
                    }
                };
                if pos.sub(c).length() < r + p.radius {
                    let d = pos.sub(render.0).length();
                    if hit.is_none_or(|(hd, _, _)| d < hd) {
                        hit = Some((d, te, part));
                    }
                }
            }
        }
        // 爆炸（M8）：aoe>0 时范围里所有目标都吃伤害（按体部算）。
        let mut victims: Vec<(Entity, HitPart)> = Vec::new();
        if let Some((_, te, part)) = hit {
            victims.push((te, part));
        }
        if p.aoe > 0.0 {
            for (te, _, render, _, _, _, _, _) in targets.iter() {
                if victims.iter().any(|(e, _)| *e == te) {
                    continue;
                }
                if pos.sub(render.0).length() <= p.aoe {
                    victims.push((te, HitPart::Body));
                }
            }
        }
        for (te, part) in victims {
            // 复用 v2 伤害管线（爆头倍率对人形生效）。
            if let Ok((entity, mut target, _, boxes, life, motion, trng, head_only)) =
                targets.get_mut(te)
            {
                if target.hp <= 0.0 {
                    continue; // 已死未 despawn（Commands 延迟）——别重复吃伤害。
                }
                let has_head = boxes
                    .0
                    .iter()
                    .any(|h| matches!(h, Hitbox::OffsetSphere { .. }));
                let head_ok =
                    head_only.is_none() || !has_head || part == HitPart::Head;
                if target.team == Team::Neutral {
                    // 打错目标：扣分 + 消失（和 hitscan 一致）。
                    score.misses += 1;
                    score.score -= launch.scenario.scoring.loss_per_miss as f64;
                    spawn.commands.entity(entity).despawn();
                    trace.file.world.push(WorldEvent {
                        t_us,
                        kind: WorldKind::Despawn,
                        id: target.id,
                        pos: V3::ZERO,
                        radius: 0.0,
                    });
                    continue;
                }
                if head_ok {
                    let dmg = p.damage
                        * if part == HitPart::Head {
                            launch.scenario.weapon.headshot_mult.max(1.0)
                        } else {
                            1.0
                        };
                    score.hits += 1;
                    if part == HitPart::Head {
                        score.headshots += 1;
                    }
                    play_sound(&audio, true);
                    if let (Some(mut m), Some(mut r)) = (motion, trng) {
                        m.0.on_hit(director.sim_time, &mut r.0);
                    }
                    target.hp -= dmg;
                    score.damage += dmg as f64;
                    let tid = target.id;
                    if target.hp <= 0.0 {
                        score.kills += 1;
                        spawn.commands.entity(entity).despawn();
                        trace.file.world.push(WorldEvent {
                            t_us,
                            kind: WorldKind::Killed,
                            id: tid,
                            pos: V3::ZERO,
                            radius: 0.0,
                        });
                        if let Some(life) = life {
                            let (lo, hi) =
                                director.groups[life.group as usize].rule.delay;
                            director.schedule(
                                life.group as usize,
                                sample_delay(&mut spawn.rng.0, lo, hi),
                            );
                        }
                    }
                    trace.file.shots.push(shot_hit_event(
                        t_us, tid, &look, pos, dmg, part, &wstate, (0.0, 0.0), body.vel,
                    ));
                }
            }
            spawn.commands.entity(pe).despawn();
            continue;
        }
        if t - p.born > 10.0 || pos.y < -2.0 || pos.z > 8.0 {
            // 到期未中：记一次 miss（出手时没记事件）。
            score.misses += 1;
            trace.file.shots.push(shot_meta_event(
                t_us, &look, &wstate, (0.0, 0.0), body.vel, false,
            ));
            spawn.commands.entity(pe).despawn();
        }
    }
}

/// 武器杂项 tick（FixedUpdate）：换弹完成、手动 R、热度/后坐衰减。
pub(crate) fn weapon_tick(
    mut w: ResMut<WeaponState>,
    director: Res<SpawnDirector>,
    keyboard: Res<ButtonInput<KeyCode>>,
    launch: Res<ViewportLaunch>,
) {
    const DT: f64 = 1.0 / 240.0;
    let t = director.sim_time as f64;
    let spec = &launch.scenario.weapon;
    if w.reload_end > 0.0 && t >= w.reload_end {
        w.reload_end = 0.0;
        w.mag_left = spec.magazine.map(|m| m as i32).unwrap_or(-1);
    }
    if let Some(mag) = spec.magazine
        && keyboard.just_pressed(KeyCode::KeyR)
        && w.reload_end <= t
        && w.mag_left < mag as i32
    {
        w.reload_end = t + spec.reload_partial.max(0.05) as f64;
    }
    if spec.spread.decay > 0.0 {
        w.heat = (w.heat - spec.spread.decay * DT as f32).max(0.0);
    }
    let reset = spec.recoil.reset_dps * DT as f32;
    if reset > 0.0 && (w.acc_pitch != 0.0 || w.acc_yaw != 0.0) {
        w.acc_pitch = (w.acc_pitch - reset).max(0.0);
        w.acc_yaw = if w.acc_yaw.abs() <= reset {
            0.0
        } else {
            w.acc_yaw - reset * w.acc_yaw.signum()
        };
        w.punch_pitch = w.acc_pitch * spec.recoil.view_scale;
        w.punch_yaw = w.acc_yaw * spec.recoil.view_scale;
    }
}

fn sample_delay(rng: &mut rand_chacha::ChaCha8Rng, lo: f32, hi: f32) -> f32 {
    use rand::Rng;
    if hi <= lo {
        lo
    } else {
        lo + rng.random::<f32>() * (hi - lo)
    }
}

fn shot_hit_event(
    t_us: u64,
    target_id: u32,
    look: &Look,
    pos: V3,
    damage: f32,
    part: HitPart,
    w: &WeaponState,
    spread_xy: (f32, f32),
    vel: V3,
) -> ShotEvent {
    ShotEvent {
        t_us,
        hit: true,
        target_id,
        yaw_deg: look.yaw,
        pitch_deg: look.pitch,
        spray_index: w.spray_index as u16,
        punch_yaw_deg: w.punch_yaw,
        punch_pitch_deg: w.punch_pitch,
        spread_x: spread_xy.0,
        spread_y: spread_xy.1,
        player_vel: vel,
        ammo_after: w.mag_left as i16,
        hits: vec![ShotHit {
            target_id,
            part: part as u8,
            damage,
            pos,
        }],
    }
}

/// 空枪 / 抛射物出手 的事件（无命中体）。
fn shot_meta_event(
    t_us: u64,
    look: &Look,
    w: &WeaponState,
    spread_xy: (f32, f32),
    vel: V3,
    hit: bool,
) -> ShotEvent {
    ShotEvent {
        t_us,
        hit,
        target_id: 0,
        yaw_deg: look.yaw,
        pitch_deg: look.pitch,
        spray_index: w.spray_index as u16,
        punch_yaw_deg: w.punch_yaw,
        punch_pitch_deg: w.punch_pitch,
        spread_x: spread_xy.0,
        spread_y: spread_xy.1,
        player_vel: vel,
        ammo_after: w.mag_left as i16,
        hits: Vec::new(),
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
    // 跟枪判定同样尊重墙体遮挡（不然 OnTargetPercent 隔墙也算）。
    let block_t = nearest_block_t(origin, dir, &spec.map.blocks);
    let mut best: Option<(u32, f32)> = None;
    for (target, render, boxes) in &targets {
        if target.team == Team::Neutral {
            continue; // 打 NoShoot 不算跟住
        }
        for hbox in boxes.0.iter() {
            if let Some((t, _part)) = hitbox_ray(origin, dir, render.0, *hbox)
                && t < block_t
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

fn miss(
    audio: &AudioBank,
    score: &mut ScoreState,
    trace: &mut LiveTrace,
    t_us: u64,
    look: &Look,
    w: &WeaponState,
    spread_xy: (f32, f32),
    vel: V3,
) {
    score.misses += 1;
    score.shots_fired += 1;
    play_sound(audio, false);
    trace.file.shots.push(shot_meta_event(
        t_us,
        look,
        w,
        spread_xy,
        vel,
        false,
    ));
}

fn play_sound(audio: &AudioBank, hit: bool) {
    let _ = audio.tx.send(if hit {
        SfxEvent::Hit
    } else {
        SfxEvent::Miss
    });
}

/// 最近障碍 AABB 的命中 t；无命中返回 MAX。
pub(crate) fn nearest_block_t(
    origin: V3,
    dir: V3,
    blocks: &[crate::sce::Block],
) -> f32 {
    let mut best = f32::MAX;
    for b in blocks {
        if let Some(t) = crate::hitscan::ray_aabb(origin, dir, b.min, b.max)
            && t < best
        {
            best = t;
        }
    }
    best
}

/// 两点间是否无遮挡（LOS）。给 require_los 出生点和 AI 视线用。
pub(crate) fn los_clear(a: V3, b: V3, blocks: &[crate::sce::Block]) -> bool {
    let d = b.sub(a);
    let dist = d.length();
    if dist < 1e-4 {
        return true;
    }
    let dir = d.scale(1.0 / dist);
    nearest_block_t(a, dir, blocks) >= dist - 1e-3
}

/// 玩家血量（M6）。
#[derive(Resource)]
pub(crate) struct PlayerHp {
    pub hp: f32,
    pub max: f32,
    /// 死亡重生后的无敌期截止（模拟秒）。
    pub invuln_until: f32,
}

/// AI 开火（FixedUpdate）：BotBrain 到点 → 视线判定 → 命中概率掉血。
/// 命中写 DamageEvent 和 hp；hp 归零记 death 并重置（M6 简化版回合）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn bot_fire(
    mut bots: Query<(&mut BotBrain, &Target, &RenderPos, Option<&mut TargetRng>)>,
    mut php: ResMut<PlayerHp>,
    body: Res<PlayerBody>,
    mut score: ResMut<ScoreState>,
    director: Res<SpawnDirector>,
    mut trace: ResMut<LiveTrace>,
    launch: Res<ViewportLaunch>,
    clock: Res<MatchClock>,
    audio: Res<AudioBank>,
    look: Res<Look>,
) {
    if clock.finished || clock.countdown > 0.0 {
        return;
    }
    use rand::Rng;
    let t = director.sim_time;
    let eye = body.eye();
    let blocks = &launch.scenario.map.blocks;
    let t_us = (t * 1_000_000.0) as u64;
    for (mut brain, _target, render, trng) in bots.iter_mut() {
        if t < brain.next_shot {
            continue;
        }
        // 再排下一发：间隔 + 重新瞄准预热。
        let (lo, hi) = brain.params.aim_time;
        let mut aim = lo;
        let mut roll = 0.5f32;
        if let Some(mut r) = trng {
            aim = if hi <= lo {
                lo
            } else {
                lo + r.0.random::<f32>() * (hi - lo)
            };
            roll = r.0.random::<f32>();
        }
        brain.next_shot = t + brain.params.fire_interval + aim;
        if !los_clear(render.0, eye, blocks) {
            continue;
        }
        // 方位音（M8）：pan = 枪声相对玩家朝向的横向分量。
        let rel = render.0.sub(eye);
        let yaw_rad = look.yaw.to_radians();
        let right_x = yaw_rad.cos();
        let right_z = yaw_rad.sin();
        let pan = (rel.x * right_x + rel.z * right_z)
            / rel.length().max(0.01)
            * 0.8;
        let _ = audio.tx.send(crate::sfx::SfxEvent::BotShot {
            pan: pan.clamp(-1.0, 1.0),
        });
        if php.invuln_until > t {
            continue;
        }
        if roll < brain.params.accuracy {
            php.hp -= brain.params.damage;
            score.damage_taken += f64::from(brain.params.damage);
            let _ = audio.tx.send(crate::sfx::SfxEvent::Damaged);
            let to_bot = render.0.sub(eye);
            let dir_yaw = to_bot.x.atan2(-to_bot.z).to_degrees();
            trace.file.damage_taken.push(crate::session::DamageEvent {
                t_us,
                amount: brain.params.damage,
                direction_yaw_deg: dir_yaw,
                source_id: _target.id,
            });
            if php.hp <= 0.0 {
                score.deaths += 1;
                php.hp = php.max;
                php.invuln_until = t + 1.5;
            }
        }
    }
}
