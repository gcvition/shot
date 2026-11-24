//! 目标实体和生成辅助。
//!
//! 一个目标实体由三块拼成：
//! - [`Target`]：身份（id、原型、血量、队伍）
//! - [`SimPos`]：模拟位置（M1 起每 fixed tick 更新，`prev`/`curr` 给渲染插值用）
//! - [`Hitboxes`]：命中判定区域（球 / 胶囊 / 盒子）
//!
//! `Transform` 只是**渲染**位置；命中判定一律读 [`SimPos`]，
//! M1 起读插值后的位置（所见即所得）。

use std::collections::HashMap;

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use rand_chacha::ChaCha8Rng;
use smallvec::SmallVec;

use crate::scenario::grid_cell_center_n;
use crate::vec3::Vec3 as V3;

/// 目标长什么样。决定命中体形状和渲染网格。非 `Sphere` 的变体 M3 起用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TargetArchetype {
    /// 球（现有点击/跟枪目标，KovaaK `MainBBType=Spheroid`）。
    Sphere,
    /// 身体胶囊 + 头部球（M3，爆头线/对枪）。
    Humanoid,
    /// AABB 方块（格子、Tile 类）。
    Box,
    /// 面向玩家的圆盘（微调类）。
    Disc,
}

/// 目标属于哪一阵营。AI 对枪场景（M6）用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Team {
    Enemy,
    Player,
    Neutral,
}

/// 活着的目标。命中判定用 [`Hitboxes`]，血量归零才 `Despawn`（M3 起）。
/// `archetype`/`hp`/`max_hp`/`team` 在 M3 人形目标和 M6 AI 才读。
#[derive(Component)]
#[allow(dead_code)]
pub(crate) struct Target {
    pub id: u32,
    pub archetype: TargetArchetype,
    pub hp: f32,
    pub max_hp: f32,
    pub team: Team,
}

/// 模拟位置（crate::vec3::Vec3，米）。静态目标 `prev == curr`；
/// `prev` 由 M1 运动插值开始读。
#[derive(Component, Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct SimPos {
    pub prev: V3,
    pub curr: V3,
}

impl SimPos {
    pub(crate) fn at(pos: V3) -> Self {
        Self { prev: pos, curr: pos }
    }
}

/// 一个命中区域。球心在 `RenderPos`；`OffsetSphere`/`Capsule` 相对它平移。
#[derive(Clone, Copy, Debug)]
pub(crate) enum Hitbox {
    /// 球心在 `RenderPos`。
    Sphere { radius: f32 },
    /// 球心在 `RenderPos + offset`（人形头部）。
    OffsetSphere { radius: f32, offset: V3 },
    /// 竖直胶囊，`center = RenderPos + (0, offset_y, 0)`（人形躯干）。
    Capsule { radius: f32, half_height: f32, offset_y: f32 },
}

/// 只有爆头计分（ShotHeadshotOnly=1）。打中身体按空枪处理。
#[derive(Component)]
pub(crate) struct HeadOnly;

/// AI 对手的大脑（M6）：到点朝玩家开枪。
#[derive(Component)]
pub(crate) struct BotBrain {
    pub params: crate::sce::BotParams,
    /// 下一次开火的模拟秒（含瞄准预热）。
    pub next_shot: f32,
}

/// 运动模型实例（v2 目标才有）。fixed tick 由 `sim::director` 推进。
#[derive(Component)]
pub(crate) struct TargetMotion(pub crate::sim::motion::Motion);

/// 该目标自己的派生 RNG（`seed ⊕ hash(id)`）。和场景 RNG 分开，
/// 保证目标 A 先死不会改变目标 B 的随机序列。
#[derive(Component)]
pub(crate) struct TargetRng(pub ChaCha8Rng);

/// 运动学状态：pos 镜像到 [`SimPos::curr`]，vel 给 Dodge/Strafe 用。
#[derive(Component, Clone, Copy)]
pub(crate) struct Kinematics(pub crate::sim::motion::KinState);

/// 渲染插值后的位置。**命中判定读这里**（所见即所得）；
/// `SimPos.prev/curr` 是模拟位置，`sync_render_pos` 每帧算出本值。
#[derive(Component, Clone, Copy)]
pub(crate) struct RenderPos(pub V3);

/// 生命周期（v2）。`expires_at` 是相对局内模拟时间的到期秒。
#[derive(Component, Clone, Copy)]
pub(crate) struct Life {
    /// 出生的局内模拟秒（反应时间统计用）。
    pub spawned_at: f32,
    /// None = 打到死。
    pub expires_at: Option<f32>,
    /// 属于哪个 spawn group（死后按该组规则重生）。
    pub group: u32,
    /// 是否已被命中过（反应时间只记第一次）。
    pub first_hit_done: bool,
}

/// 该目标的活动范围（由其 spawn region 外扩得到）。
#[derive(Component, Clone, Copy)]
pub(crate) struct TargetBounds(pub crate::sim::motion::Aabb);

/// 目标的命中区域集合（人形是 身体+头 两块）。
#[derive(Component)]
pub(crate) struct Hitboxes(pub SmallVec<[Hitbox; 3]>);

/// 按「原型 + 尺寸」缓存网格。回放按 WorldEvent 重建目标时不再每帧 `meshes.add`。
#[derive(Resource, Default)]
pub(crate) struct MeshCache {
    spheres: HashMap<u32, Handle<Mesh>>,
}

impl MeshCache {
    /// 球的细分按半径分档（和旧 `spawn_sphere` 一致：大球 40×20，小球 24×12）。
    pub(crate) fn sphere(&mut self, meshes: &mut Assets<Mesh>, radius: f32) -> Handle<Mesh> {
        self.spheres
            .entry(radius.to_bits())
            .or_insert_with(|| {
                let (sectors, stacks) = if radius > 0.12 { (40, 20) } else { (24, 12) };
                meshes.add(Sphere::new(radius).mesh().uv(sectors, stacks))
            })
            .clone()
    }
}

/// 目标用的材质表。`no_shoot` 是 Go/No-Go 的「别打」目标色。
#[derive(Resource)]
#[allow(dead_code)]
pub(crate) struct TargetMaterials {
    pub normal: Handle<StandardMaterial>,
    pub hit_flash: Option<Handle<StandardMaterial>>,
    pub head: Option<Handle<StandardMaterial>>,
    pub no_shoot: Option<Handle<StandardMaterial>>,
}

/// 场景级 RNG。回放用轨迹里的种子，保证出生点复现。
#[derive(Resource)]
pub(crate) struct RngRes(pub ChaCha8Rng);

/// 下一个随机墙面目标的 id。格子模式用格子下标当 id，不走这个。
#[derive(Resource)]
pub(crate) struct NextTargetId(pub u32);

/// 「生成一个新目标」要摸的资源打包。Bevy 单个 system 最多 16 个参数，
/// 装不下的就往这里塞。
#[derive(SystemParam)]
pub(crate) struct SpawnCtx<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub rng: ResMut<'w, RngRes>,
    pub next_id: ResMut<'w, NextTargetId>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub cache: ResMut<'w, MeshCache>,
    pub mats: Res<'w, TargetMaterials>,
}

/// 把一个球目标放进 `layer`。`id` 给 hitscan 用，不要和别的活目标重复。
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_sphere(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    cache: &mut MeshCache,
    mat: Handle<StandardMaterial>,
    id: u32,
    pos: V3,
    radius: f32,
    layer: RenderLayers,
) {
    let mesh = cache.sphere(meshes, radius);
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        Transform::from_xyz(pos.x, pos.y, pos.z),
        Target {
            id,
            archetype: TargetArchetype::Sphere,
            hp: 1.0,
            max_hp: 1.0,
            team: Team::Enemy,
        },
        SimPos::at(pos),
        RenderPos(pos),
        Hitboxes(SmallVec::from_vec(vec![Hitbox::Sphere { radius }])),
        layer,
    ));
}

/// v2 目标：带运动、派生 RNG、生命周期。`no_shoot` 目标染 Neutral 阵营。
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_moving(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    cache: &mut MeshCache,
    mat: Handle<StandardMaterial>,
    id: u32,
    pos: V3,
    radius: f32,
    layer: RenderLayers,
    motion: crate::sim::motion::Motion,
    rng: ChaCha8Rng,
    bounds: crate::sim::motion::Aabb,
    life: Life,
    no_shoot: bool,
    hp: f32,
    headshot_only: bool,
    archetype: TargetArchetype,
) -> Entity {
    let mesh = cache.sphere(meshes, radius);
    let mut e = commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        Transform::from_xyz(pos.x, pos.y, pos.z),
        Target {
            id,
            archetype,
            hp: if hp > 0.0 { hp } else { 1.0 },
            max_hp: if hp > 0.0 { hp } else { 1.0 },
            team: if no_shoot { Team::Neutral } else { Team::Enemy },
        },
        SimPos::at(pos),
        RenderPos(pos),
        Hitboxes(SmallVec::from_vec(vec![Hitbox::Sphere { radius }])),
        TargetMotion(motion),
        TargetRng(rng),
        Kinematics(crate::sim::motion::KinState {
            pos,
            vel: V3::ZERO,
            on_ground: true,
        }),
        TargetBounds(bounds),
        life,
        layer,
    ));
    if headshot_only {
        e.insert(HeadOnly);
    }
    e.id()
}

/// 人形目标（M3）：身体竖直胶囊 + 头部小球。
/// 锚点 `pos` = 胶囊中心（与球形目标一致，运动模型直接驱动）。
/// 命中体：躯干胶囊（offset_y=0）+ 头 `OffsetSphere`(+HEAD_Y)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_humanoid(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mat: Handle<StandardMaterial>,
    head_mat: Handle<StandardMaterial>,
    id: u32,
    pos: V3,
    layer: RenderLayers,
    motion: crate::sim::motion::Motion,
    rng: ChaCha8Rng,
    bounds: crate::sim::motion::Aabb,
    life: Life,
    no_shoot: bool,
    hp: f32,
    headshot_only: bool,
) -> Entity {
    const BODY_R: f32 = 0.28;
    const BODY_HALF: f32 = 0.58;
    const HEAD_R: f32 = 0.16;
    const HEAD_Y: f32 = 1.02;
    let body = meshes.add(Capsule3d::new(BODY_R, BODY_HALF * 2.0));
    let head = meshes.add(Sphere::new(HEAD_R).mesh().uv(24, 12));
    let hp = if hp > 0.0 { hp } else { 1.0 };
    let mut e = commands.spawn((
        Mesh3d(body),
        MeshMaterial3d(mat),
        Transform::from_xyz(pos.x, pos.y, pos.z),
        Target {
            id,
            archetype: TargetArchetype::Humanoid,
            hp,
            max_hp: hp,
            team: if no_shoot { Team::Neutral } else { Team::Enemy },
        },
        SimPos::at(pos),
        RenderPos(pos),
        Hitboxes(SmallVec::from_vec(vec![
            Hitbox::Capsule {
                radius: BODY_R,
                half_height: BODY_HALF,
                offset_y: 0.0,
            },
            Hitbox::OffsetSphere {
                radius: HEAD_R,
                offset: V3::new(0.0, HEAD_Y, 0.0),
            },
        ])),
        TargetMotion(motion),
        TargetRng(rng),
        Kinematics(crate::sim::motion::KinState {
            pos,
            vel: V3::ZERO,
            on_ground: true,
        }),
        TargetBounds(bounds),
        life,
        layer.clone(),
    ));
    if headshot_only {
        e.insert(HeadOnly);
    }
    // 头部视觉球（子实体跟随父 Transform）。
    e.with_child((
        Mesh3d(head),
        MeshMaterial3d(head_mat),
        Transform::from_xyz(0.0, HEAD_Y, 0.0),
        layer,
    ));
    e.id()
}

/// `cells` 格子的列数（9 → 3，16 → 4）。
pub(crate) fn grid_cols(cells: usize) -> usize {
    ((cells as f32).sqrt().round() as usize).max(1)
}

/// 第 `index` 个格子、实际半径 `radius` 的球心。
pub(crate) fn grid_pos(index: usize, cells: usize, radius: f32) -> V3 {
    let mut pos = grid_cell_center_n(index, grid_cols(cells));
    pos.z = crate::scenario::GRID_Z + radius;
    pos
}
