//! v2 场景的运行时：fixed-tick 目标运动、渲染插值、出生导演。
//!
//! # 为什么分 fixed 和 render 两套位置
//!
//! 模拟在 `FixedUpdate`（[`SIM_HZ`] = 240Hz）跑：每个 tick 把
//! `SimPos.prev = curr`，推进 [`TargetMotion`] 得新 `curr`。
//! 渲染帧间隔和 240Hz 对不齐，所以 `Update` 里的
//! [`sync_render_pos`] 按 `Time<Fixed>::overstep_fraction` 插值出
//! [`RenderPos`]。**命中判定一律读 `RenderPos`**——所见即所得，
//! 240Hz 下玩家看到的和打到的差 < 1cm。
//!
//! # 确定性
//!
//! 每个目标的运动 RNG 是 `seed ⊕ hash(id)` 派生的独立流（[`derive_rng`]），
//! 场景 RNG（`RngRes`）只管出生点采样。目标死亡顺序不影响彼此的随机序列，
//! 同种子回放逐位一致。

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;

use crate::sce::{SpawnGroup, SpawnRegion};
use crate::scenario::PLAYER_EYE;
use crate::session::{TargetKeyframe, WorldEvent, WorldKind};
use crate::sim::motion::{Aabb, MotionCtx};
use crate::vec3::Vec3 as V3;

use super::targets::{Life, MeshCache, SpawnCtx, TargetBounds};
use super::{
    Kinematics, LiveTrace, Look, PlayerBody, RenderPos, SimPos, Target, TargetMotion, TargetRng,
    ViewportLaunch, WORLD_LAYER,
};

/// 固定模拟频率。240Hz 时即使 360FPS 显示器插值误差也 < 1cm。
pub(crate) const SIM_HZ: f64 = 240.0;
/// 轨迹里目标关键帧的频率（回放插值用）。
const KEYFRAME_SECS: f32 = 1.0 / 60.0;
/// 出生采点最多试这么多次（最小间距 / 准星夹角约束）。
const SAMPLE_TRIES: usize = 16;

/// 一个待出生的目标（死后按组规则延迟重生）。
#[derive(Clone, Copy, Debug)]
pub(crate) struct PendingSpawn {
    pub group: usize,
    /// 在 `sim_time` 到达该值时出生。
    pub at: f32,
}

/// v2 场景的出生导演。legacy 场景为空壳（`groups` 为空，什么都不做）。
#[derive(Resource)]
pub(crate) struct SpawnDirector {
    /// `spec.spawns` 的运行时拷贝。
    pub groups: Vec<SpawnGroup>,
    /// 排队中的重生。
    pub pending: Vec<PendingSpawn>,
    /// 局内模拟时间（秒，Playing 起算）。
    pub sim_time: f32,
    /// 派生目标 RNG 用的底种子（轨迹 header 里的同一个）。
    pub base_seed: u64,
    keyframe_acc: f32,
}

impl SpawnDirector {
    pub(crate) fn new(groups: Vec<SpawnGroup>, base_seed: u64) -> Self {
        Self {
            groups,
            pending: Vec::new(),
            sim_time: 0.0,
            base_seed,
            keyframe_acc: 0.0,
        }
    }

    /// 目标死后排一个重生。`delay` 已采样好的秒数。
    pub(crate) fn schedule(&mut self, group: usize, delay: f32) {
        self.pending.push(PendingSpawn {
            group,
            at: self.sim_time + delay,
        });
    }
}

/// `seed ⊕ hash(id)` 派生单个目标的运动 RNG。
pub(crate) fn derive_rng(base_seed: u64, id: u32) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(
        base_seed ^ (id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
    )
}

/// 由 spawn region 外扩 `margin` 得到该目标的活动范围。
pub(crate) fn region_aabb(region: &SpawnRegion, margin: f32) -> Aabb {
    let (min, max) = match region {
        SpawnRegion::WallRect { center, size } => (
            V3::new(
                center.x - size.x * 0.5,
                center.y - size.y * 0.5,
                center.z - size.z * 0.5,
            ),
            V3::new(
                center.x + size.x * 0.5,
                center.y + size.y * 0.5,
                center.z + size.z * 0.5,
            ),
        ),
        SpawnRegion::Grid {
            origin,
            cols,
            rows,
            spacing,
        } => (
            *origin,
            V3::new(
                origin.x + (*cols as f32 - 1.0).max(0.0) * spacing.x,
                origin.y + (*rows as f32 - 1.0).max(0.0) * spacing.y,
                origin.z + (*cols as f32 - 1.0).max(0.0) * spacing.z,
            ),
        ),
        SpawnRegion::Box { min, max } => (*min, *max),
        SpawnRegion::AngularShell {
            yaw_range: _,
            pitch_range: _,
            dist_range,
        } => {
            let d = dist_range.1;
            (
                V3::new(
                    PLAYER_EYE.x - d,
                    (PLAYER_EYE.y - d).max(0.0),
                    PLAYER_EYE.z - d,
                ),
                V3::new(PLAYER_EYE.x + d, PLAYER_EYE.y + d, PLAYER_EYE.z + d),
            )
        }
        SpawnRegion::Points(points) => {
            let mut min = points.first().map(|p| p.pos).unwrap_or(V3::ZERO);
            let mut max = min;
            for p in points {
                min = V3::new(min.x.min(p.pos.x), min.y.min(p.pos.y), min.z.min(p.pos.z));
                max = V3::new(max.x.max(p.pos.x), max.y.max(p.pos.y), max.z.max(p.pos.z));
            }
            (min, max)
        }
        SpawnRegion::CoverEdges { .. } => (V3::new(-20.0, 0.0, -40.0), V3::new(20.0, 8.0, 5.0)),
    };
    Aabb {
        min: V3::new(min.x - margin, min.y - margin, min.z - margin),
        max: V3::new(max.x + margin, max.y + margin, max.z + margin),
    }
}

/// 造一个 v2 目标实体（含运动/RNG/生命周期），并写 WorldEvent::Spawn。
/// 参数故意散开传（不用 `SpawnCtx`）：`setup` 里不是 system，手搓不了 SystemParam。
#[allow(clippy::too_many_arguments)]
fn spawn_group_target(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    cache: &mut MeshCache,
    normal_mat: &Handle<StandardMaterial>,
    no_shoot_mat: Option<&Handle<StandardMaterial>>,
    rng: &mut ChaCha8Rng,
    next_id: &mut u32,
    director: &SpawnDirector,
    group_idx: usize,
    pos: V3,
    sim_time: f32,
    t_us: u64,
    trace: &mut crate::session::SessionFile,
) {
    let group = &director.groups[group_idx];
    let id = *next_id;
    *next_id += 1;
    let radius = if group.radius > 0.0 { group.radius } else { 0.3 };
    let mat = if group.no_shoot {
        no_shoot_mat.cloned().unwrap_or_else(|| normal_mat.clone())
    } else {
        normal_mat.clone()
    };
    let mut motion_rng = derive_rng(director.base_seed, id);
    let motion = group.motion.to_motion(pos, &mut motion_rng);
    let expires_at = group.rule.lifetime.map(|(lo, hi)| {
        sim_time
            + if hi <= lo {
                lo
            } else {
                lo + rng.random::<f32>() * (hi - lo)
            }
    });
    let bounds = region_aabb(&group.region, 3.0);
    let life = Life {
        spawned_at: sim_time,
        expires_at,
        group: group_idx as u32,
        first_hit_done: false,
    };
    let spawned = if group.archetype == crate::sce::TargetShape::Humanoid {
        // 人形：胶囊身体 + 头；head 材质暂用 normal。
        super::targets::spawn_humanoid(
            commands,
            meshes,
            mat.clone(),
            mat.clone(),
            id,
            pos,
            RenderLayers::layer(WORLD_LAYER),
            motion,
            motion_rng,
            bounds,
            life,
            group.no_shoot,
            group.hp,
            group.headshot_only,
        )
    } else {
        let archetype = match group.archetype {
            crate::sce::TargetShape::Box => super::targets::TargetArchetype::Box,
            crate::sce::TargetShape::Disc => super::targets::TargetArchetype::Disc,
            _ => super::targets::TargetArchetype::Sphere,
        };
        super::targets::spawn_moving(
            commands,
            meshes,
            cache,
            mat,
            id,
            pos,
            radius,
            RenderLayers::layer(WORLD_LAYER),
            motion,
            motion_rng,
            bounds,
            life,
            group.no_shoot,
            group.hp,
            group.headshot_only,
            archetype,
        )
    };
    // AI 对手：装大脑，首发瞄准时间用场景 RNG 采样。
    if let Some(bot) = group.bot {
        let (lo, hi) = bot.aim_time;
        let first = sim_time
            + if hi <= lo { lo } else { lo + rng.random::<f32>() * (hi - lo) };
        commands.entity(spawned).insert(super::targets::BotBrain {
            params: bot,
            next_shot: first,
        });
    }
    trace.world.push(WorldEvent {
        t_us,
        kind: WorldKind::Spawn,
        id,
        pos,
        radius,
    });
}

/// FixedUpdate：推进所有运动目标一个 tick，写 keyframe，处理寿命到期。
/// 只在 `RunState::Playing` 注册。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn sim_targets(
    fixed: Res<Time<Fixed>>,
    mut director: ResMut<SpawnDirector>,
    mut trace: ResMut<LiveTrace>,
    mut commands: Commands,
    mut scene_rng: ResMut<super::RngRes>,
    mut query: Query<(
        Entity,
        &Target,
        &mut SimPos,
        &mut Kinematics,
        &mut TargetMotion,
        &mut TargetRng,
        &TargetBounds,
        &Life,
    )>,
) {
    let dt = fixed.delta_secs();
    director.sim_time += dt;
    let ctx = MotionCtx {
        player_eye: PLAYER_EYE,
        bounds: Aabb::default(),
        time: director.sim_time,
    };
    let t_us = (director.sim_time * 1_000_000.0) as u64;
    let write_keyframe = {
        director.keyframe_acc += dt;
        if director.keyframe_acc >= KEYFRAME_SECS {
            director.keyframe_acc -= KEYFRAME_SECS;
            true
        } else {
            false
        }
    };
    for (entity, target, mut sim, mut kin, mut motion, mut trng, bounds, life) in &mut query {
        sim.prev = sim.curr;
        let mut c = ctx;
        c.bounds = bounds.0;
        motion.0.step(&mut kin.0, &c, &mut trng.0, dt);
        sim.curr = kin.0.pos;
        if let Some(at) = life.expires_at
            && director.sim_time >= at
        {
            // 寿终：消失 + 按组规则排重生（延迟区间用场景 RNG 采样）。
            commands.entity(entity).despawn();
            trace.file.world.push(WorldEvent {
                t_us,
                kind: WorldKind::Despawn,
                id: target.id,
                pos: V3::ZERO,
                radius: 0.0,
            });
            let (lo, hi) = director.groups[life.group as usize].rule.delay;
            let delay = if hi <= lo {
                lo
            } else {
                lo + scene_rng.0.random::<f32>() * (hi - lo)
            };
            director.schedule(life.group as usize, delay);
            continue;
        }
        if write_keyframe {
            trace.file.targets.push(TargetKeyframe {
                t_us,
                id: target.id,
                pos: sim.curr,
                hp: target.hp,
            });
        }
    }
}

/// Update：`prev`/`curr` 按 fixed 剩余进度插值出 [`RenderPos`] 和 Transform。
/// 静态目标 `prev == curr`，插值是恒等操作（legacy 零开销零差异）。
pub(crate) fn sync_render_pos(
    fixed: Res<Time<Fixed>>,
    mut query: Query<(&SimPos, &mut RenderPos, &mut Transform)>,
) {
    let alpha = fixed.overstep_fraction();
    for (sim, mut render, mut tf) in &mut query {
        let pos = sim
            .prev
            .add(sim.curr.sub(sim.prev).scale(alpha));
        render.0 = pos;
        tf.translation = Vec3::new(pos.x, pos.y, pos.z);
    }
}

/// 出生点采样：区域采点 + 最小间距 + 准星夹角约束，最多试 [`SAMPLE_TRIES`] 次。
fn sample_point(
    group: &SpawnGroup,
    rng: &mut ChaCha8Rng,
    live: &[V3],
    look_dir: V3,
    eye: V3,
    blocks: &[crate::sce::Block],
) -> V3 {
    let mut last = V3::ZERO;
    for _ in 0..SAMPLE_TRIES {
        let pos = group.region.sample(rng, eye);
        last = pos;
        // require_los：出生点必须看得见。
        if group.rule.require_los
            && !super::combat::los_clear(eye, pos, blocks)
        {
            continue;
        }
        let sep_ok = group.rule.min_separation <= 0.0
            || live
                .iter()
                .all(|p| p.distance(pos) >= group.rule.min_separation);
        if !sep_ok {
            continue;
        }
        let rule = &group.rule;
        if rule.min_angle_from_crosshair_deg > 0.0
            || rule.max_angle_from_crosshair_deg < 180.0
        {
            let rel = pos.sub(eye);
            if rel.length() < 1e-3 {
                continue; // 采样点贴在眼位 → 方向无定义，拒绝。
            }
            let to = rel.normalized();
            let cos = to.dot(look_dir).clamp(-1.0, 1.0);
            let ang = cos.acos().to_degrees();
            if ang < rule.min_angle_from_crosshair_deg
                || ang > rule.max_angle_from_crosshair_deg
            {
                continue;
            }
        }
        return pos;
    }
    last
}

/// Update：冲刷到期的 pending，按组规则补目标。
/// `max_alive` 与组 `count` 同时生效；alive 统计含 pending 占位。
pub(crate) fn director_tick(
    mut director: ResMut<SpawnDirector>,
    mut spawn: SpawnCtx,
    mut trace: ResMut<LiveTrace>,
    look: Res<Look>,
    body: Res<PlayerBody>,
    launch: Res<ViewportLaunch>,
    _time: Res<Time>,
    targets: Query<(&Target, &SimPos, Option<&Life>)>,
) {
    if director.groups.is_empty() {
        return;
    }
    let t_us = (director.sim_time * 1_000_000.0) as u64;
    let look_dir = crate::scenario::look_direction(look.yaw, look.pitch);
    // 收集到期任务（先拿出来，避免借用冲突）。
    let now = director.sim_time;
    let mut due = Vec::new();
    director.pending.retain(|p| {
        if p.at <= now {
            due.push(*p);
            false
        } else {
            true
        }
    });
    // 本 pass 已生的目标对后续 alive/minSep 判定可见（Commands 延迟，Query 看不到）。
    let mut live: Vec<V3> = targets.iter().map(|(_, sim, _)| sim.curr).collect();
    let mut extra_alive: std::collections::HashMap<usize, usize> = Default::default();
    for p in due {
        let Some(group) = director.groups.get(p.group) else {
            continue;
        };
        let alive = targets
            .iter()
            .filter(|(_, _, life)| life.is_some_and(|l| l.group as usize == p.group))
            .count()
            + extra_alive.get(&p.group).copied().unwrap_or(0);
        if alive >= group.count.min(group.rule.max_alive) {
            continue;
        }
        let pos = sample_point(
            group,
            &mut spawn.rng.0,
            &live,
            look_dir,
            body.eye(),
            &launch.scenario.map.blocks,
        );
        live.push(pos);
        *extra_alive.entry(p.group).or_insert(0) += 1;
        spawn_group_target(
            &mut spawn.commands,
            &mut spawn.meshes,
            &mut spawn.cache,
            &spawn.mats.normal,
            spawn.mats.no_shoot.as_ref(),
            &mut spawn.rng.0,
            &mut spawn.next_id.0,
            &director,
            p.group,
            pos,
            director.sim_time,
            t_us,
            &mut trace.file,
        );
    }
}

/// setup 调用：v2 场景的首批目标直接出生（不吃 delay），写 t=0 的 Spawn 事件。
/// 组内也做最小间距约束，避免开局两个球叠一起。
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_initial(
    director: &SpawnDirector,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    cache: &mut MeshCache,
    normal_mat: &Handle<StandardMaterial>,
    no_shoot_mat: Option<&Handle<StandardMaterial>>,
    rng: &mut ChaCha8Rng,
    next_id: &mut u32,
    trace: &mut crate::session::SessionFile,
    look_dir: V3,
    eye: V3,
    blocks: &[crate::sce::Block],
) {
    let mut live: Vec<V3> = Vec::new();
    for gi in 0..director.groups.len() {
        for _ in 0..director.groups[gi].count.min(director.groups[gi].rule.max_alive) {
            let pos = sample_point(&director.groups[gi], rng, &live, look_dir, eye, blocks);
            spawn_group_target(
                commands, meshes, cache, normal_mat, no_shoot_mat, rng, next_id, director,
                gi, pos, 0.0, 0, trace,
            );
            live.push(pos);
        }
    }
}
