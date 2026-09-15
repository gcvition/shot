//! 目标生成的纯数学，不碰 Bevy。
//!
//! 坐标系（右手，Y 向上）：玩家眼睛在 [`PLAYER_EYE`]，瞄准墙在 `z = WALL_Z`（约 -7.2）。
//!
//! - **随机墙**（Sixshot）：在墙矩形里随机撒点，彼此至少 [`MIN_SEPARATION`]。
//!   球很小（[`TARGET_RADIUS`] = 5cm），无光照。
//! - **格子**（Gridshot）：3×3（或更大）网格，同时只活 [`GRID_LIVE`] 个。
//!   球更大（[`GRID_RADIUS`]），有金属高光。
//!
//! [`look_direction`] 把 yaw/pitch（度）变成单位视线。viewport 的相机和 hitscan 都用它。

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::vec3::Vec3;

/// Sixshot 那种小黑点的半径（米）。
pub const TARGET_RADIUS: f32 = 0.05;
/// 随机墙面上两个球心最少隔这么远，避免重叠到打不中。
pub const MIN_SEPARATION: f32 = 0.50;
/// 玩家眼睛。相机和射线原点都用这个，不要改成 (0,0,0)。
pub const PLAYER_EYE: Vec3 = Vec3::new(0.0, 1.62, 0.0);
/// 瞄准墙的世界 Z。房间「前面」也贴在这里。
pub const WALL_Z: f32 = -7.2;
pub const GRID_Z: f32 = WALL_Z;
/// 随机墙左右边界（米，相对房间中心）。
pub const WALL_X: f32 = 2.15;
pub const WALL_Y_MIN: f32 = 1.15;
pub const WALL_Y_MAX: f32 = 4.05;
pub const SIX_TARGET_COUNT: usize = 6;
/// Gridshot 默认同时活着的球数。
pub const GRID_LIVE: usize = 3;
pub const GRID_RADIUS: f32 = 0.28;
pub const GRID_SPACING: f32 = 1.20;
pub const GRID_ORIGIN_Y: f32 = 1.47;

/// 一个目标的出生姿势。随机墙的 `id` 递增；格子模式的 `id` 等于格子下标。
#[derive(Clone, Copy, Debug)]
pub struct TargetPose {
    pub id: u32,
    pub pos: Vec3,
    pub radius: f32,
}

/// 在墙矩形里随机一个不和 `existing` 太近的点。200 次失败就回退到墙心。
pub fn sample_unique_point(
    rng: &mut ChaCha8Rng,
    existing: &[Vec3],
    radius: f32,
    min_sep: f32,
) -> Vec3 {
    let pad = radius + 0.12;
    for _ in 0..200 {
        let x = rng.random_range((-WALL_X + pad)..(WALL_X - pad));
        let y = rng.random_range((WALL_Y_MIN + pad)..(WALL_Y_MAX - pad));
        let pos = Vec3::new(x, y, WALL_Z + radius);
        if existing.iter().all(|p| p.distance(pos) >= min_sep) {
            return pos;
        }
    }
    Vec3::new(0.0, 2.0, WALL_Z + radius)
}

pub fn initial_random_targets(rng: &mut ChaCha8Rng, count: usize, radius: f32) -> Vec<TargetPose> {
    let mut poses = Vec::with_capacity(count);
    let mut points = Vec::new();
    for id in 0..count as u32 {
        let pos = sample_unique_point(rng, &points, radius, MIN_SEPARATION);
        points.push(pos);
        poses.push(TargetPose { id, pos, radius });
    }
    poses
}

pub fn initial_six_targets(rng: &mut ChaCha8Rng) -> Vec<TargetPose> {
    initial_random_targets(rng, SIX_TARGET_COUNT, TARGET_RADIUS)
}

/// 3×3 格子中第 `index` 个格子的球心（index 0 在左下）。
pub fn grid_cell_center(index: usize) -> Vec3 {
    grid_cell_center_n(index, 3)
}

/// 任意列数的格子中心。行从下往上排。
pub fn grid_cell_center_n(index: usize, cols: usize) -> Vec3 {
    let cols = cols.max(1);
    let col = (index % cols) as f32;
    let row = (index / cols) as f32;
    let origin_x = -GRID_SPACING * (cols.saturating_sub(1) as f32) * 0.5;
    let origin_y = GRID_ORIGIN_Y;
    Vec3::new(
        origin_x + col * GRID_SPACING,
        origin_y + row * GRID_SPACING,
        GRID_Z + GRID_RADIUS,
    )
}

pub fn initial_grid_live(rng: &mut ChaCha8Rng) -> Vec<usize> {
    initial_grid_live_n(rng, GRID_LIVE, 9)
}

/// 开局随机挑 `live` 个格子亮着。
pub fn initial_grid_live_n(rng: &mut ChaCha8Rng, live: usize, cells: usize) -> Vec<usize> {
    let mut cells: Vec<usize> = (0..cells.max(1)).collect();
    let live = live.min(cells.len()).max(1);
    for i in 0..live {
        let j = rng.random_range(i..cells.len());
        cells.swap(i, j);
    }
    cells.truncate(live);
    cells
}

pub fn next_grid_cell(rng: &mut ChaCha8Rng, live: &[usize]) -> usize {
    next_grid_cell_n(rng, live, 9)
}

/// 打掉一个格子后，从还暗着的格子里随机补一个。
pub fn next_grid_cell_n(rng: &mut ChaCha8Rng, live: &[usize], cells: usize) -> usize {
    let mut dead: Vec<usize> = (0..cells.max(1)).filter(|i| !live.contains(i)).collect();
    if dead.is_empty() {
        return 0;
    }
    let i = rng.random_range(0..dead.len());
    dead.swap_remove(i)
}

/// yaw 左右、pitch 上下（度）→ 单位视线。pitch=0 时看向 -Z。
pub fn look_direction(yaw_deg: f32, pitch_deg: f32) -> Vec3 {
    let yaw = yaw_deg.to_radians();
    let pitch = pitch_deg.to_radians();
    Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        -yaw.cos() * pitch.cos(),
    )
    .normalized()
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    use super::{
        GRID_LIVE, GRID_RADIUS, GRID_SPACING, MIN_SEPARATION, SIX_TARGET_COUNT, initial_grid_live,
        initial_six_targets, next_grid_cell,
    };

    #[test]
    fn six_targets_should_keep_count_and_separation() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let poses = initial_six_targets(&mut rng);
        assert_eq!(poses.len(), SIX_TARGET_COUNT);
        for (i, a) in poses.iter().enumerate() {
            for b in poses.iter().skip(i + 1) {
                assert!(a.pos.distance(b.pos) + 1e-4 >= MIN_SEPARATION);
            }
        }
    }

    #[test]
    fn grid_should_always_have_three_live_cells() {
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let mut live = initial_grid_live(&mut rng);
        assert_eq!(live.len(), GRID_LIVE);
        let hit = live[0];
        live.retain(|c| *c != hit);
        live.push(next_grid_cell(&mut rng, &live));
        assert_eq!(live.len(), GRID_LIVE);
        let unique = live
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), GRID_LIVE);
    }

    #[test]
    fn grid_should_place_three_by_three_spheres_on_the_far_wall() {
        let c = super::grid_cell_center(4);
        assert!((c.z - (super::GRID_Z + GRID_RADIUS)).abs() < 1e-4);
        let bottom = super::grid_cell_center(0).y - GRID_RADIUS;
        let top = super::grid_cell_center(8).y + GRID_RADIUS;
        assert!(bottom > 1.0, "bottom={bottom}");
        assert!(top < 4.5, "top={top}");
        let width = super::grid_cell_center(2).x - super::grid_cell_center(0).x;
        assert!((width - 2.0 * GRID_SPACING).abs() < 1e-4);
        assert!(c.x.abs() < 0.05);
    }
}
