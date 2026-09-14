use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::vec3::Vec3;

pub const TARGET_RADIUS: f32 = 0.25;
pub const MIN_SEPARATION: f32 = TARGET_RADIUS * 3.0;
pub const PLAYER_EYE: Vec3 = Vec3::new(0.0, 1.5, 0.0);
pub const WALL_Z: f32 = -10.0;
pub const WALL_X: f32 = 8.0;
pub const WALL_Y_MIN: f32 = 0.4;
pub const WALL_Y_MAX: f32 = 4.2;
pub const SIX_TARGET_COUNT: usize = 6;
pub const GRID_LIVE: usize = 3;
pub const GRID_CELL: f32 = 1.15;
pub const GRID_GAP: f32 = 0.12;

#[derive(Clone, Copy, Debug)]
pub struct TargetPose {
    pub id: u32,
    pub pos: Vec3,
    pub radius: f32,
}

pub fn sample_unique_point(
    rng: &mut ChaCha8Rng,
    existing: &[Vec3],
    radius: f32,
    min_sep: f32,
) -> Vec3 {
    let pad = radius + 0.15;
    for _ in 0..200 {
        let x = rng.random_range((-WALL_X + pad)..(WALL_X - pad));
        let y = rng.random_range((WALL_Y_MIN + pad)..(WALL_Y_MAX - pad));
        let pos = Vec3::new(x, y, WALL_Z);
        if existing.iter().all(|p| p.distance(pos) >= min_sep) {
            return pos;
        }
    }
    Vec3::new(0.0, 2.0, WALL_Z)
}

pub fn initial_six_targets(rng: &mut ChaCha8Rng) -> Vec<TargetPose> {
    let mut poses = Vec::with_capacity(SIX_TARGET_COUNT);
    let mut points = Vec::new();
    for id in 0..SIX_TARGET_COUNT as u32 {
        let pos = sample_unique_point(rng, &points, TARGET_RADIUS, MIN_SEPARATION);
        points.push(pos);
        poses.push(TargetPose {
            id,
            pos,
            radius: TARGET_RADIUS,
        });
    }
    poses
}

pub fn grid_cell_center(index: usize) -> Vec3 {
    let col = (index % 3) as f32;
    let row = (index / 3) as f32;
    let step = GRID_CELL + GRID_GAP;
    let origin_x = -step;
    let origin_y = 1.15;
    Vec3::new(origin_x + col * step, origin_y + row * step, WALL_Z)
}

pub fn grid_cell_aabb(index: usize) -> (Vec3, Vec3) {
    let c = grid_cell_center(index);
    let h = GRID_CELL * 0.5;
    (
        Vec3::new(c.x - h, c.y - h, c.z - 0.18),
        Vec3::new(c.x + h, c.y + h, c.z + 0.18),
    )
}

pub fn initial_grid_live(rng: &mut ChaCha8Rng) -> Vec<usize> {
    let mut cells: Vec<usize> = (0..9).collect();
    for i in 0..GRID_LIVE {
        let j = rng.random_range(i..cells.len());
        cells.swap(i, j);
    }
    cells.truncate(GRID_LIVE);
    cells
}

pub fn next_grid_cell(rng: &mut ChaCha8Rng, live: &[usize]) -> usize {
    let mut dead: Vec<usize> = (0..9).filter(|i| !live.contains(i)).collect();
    if dead.is_empty() {
        return 0;
    }
    let i = rng.random_range(0..dead.len());
    dead.swap_remove(i)
}

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
        grid_cell_aabb, initial_grid_live, initial_six_targets, next_grid_cell, GRID_LIVE,
        MIN_SEPARATION, SIX_TARGET_COUNT,
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
        let unique = live.iter().copied().collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), GRID_LIVE);
    }

    #[test]
    fn grid_aabb_should_contain_its_center() {
        let (min, max) = grid_cell_aabb(4);
        assert!(min.x < 0.0 && max.x > 0.0);
        assert!(min.y < 2.4 && max.y > 2.2);
    }
}
