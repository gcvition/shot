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

/// 一个命中区域。目前只有球；M3 加 `Capsule { radius, height, offset }`。
#[derive(Clone, Copy, Debug)]
pub(crate) enum Hitbox {
    /// 球心在 [`SimPos::curr`]。
    Sphere { radius: f32 },
}

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

/// 目标用的材质表。M0 只有 `normal`；M3 起填受击闪白和头部材质。
#[derive(Resource)]
#[allow(dead_code)]
pub(crate) struct TargetMaterials {
    pub normal: Handle<StandardMaterial>,
    pub hit_flash: Option<Handle<StandardMaterial>>,
    pub head: Option<Handle<StandardMaterial>>,
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
        Hitboxes(SmallVec::from_vec(vec![Hitbox::Sphere { radius }])),
        layer,
    ));
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
