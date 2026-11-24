//! 射线打球体 / AABB。训练实际只用 [`ray_sphere`]；AABB 留给以后的盒子目标。
//!
//! 返回值是沿射线的 `t`（原点 + dir * t = 交点）。取最小正 t 就是最近命中。

use crate::vec3::Vec3;

/// 射线与球。`dir` 会被单位化。原点在球内时返回离开球面的那个 t。
pub fn ray_sphere(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let dir = dir.normalized();
    let oc = origin.sub(center);
    let b = oc.dot(dir);
    let c = oc.dot(oc) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let t0 = -b - s;
    let t1 = -b + s;
    if t0 > 1e-4 {
        Some(t0)
    } else if t1 > 1e-4 {
        Some(t1)
    } else {
        None
    }
}

/// 射线与竖直胶囊（人形躯干）。 = 胶囊中点， = 圆柱段半高
/// （不含两端半球帽，总高 = 2*(half_height + radius)）。原点在胶囊内返回 None。
pub fn ray_capsule(
    origin: Vec3,
    dir: Vec3,
    center: Vec3,
    radius: f32,
    half_height: f32,
) -> Option<f32> {
    let dir = dir.normalized();
    // 分解到「柱轴 Y」与「径向平面 XZ」。
    let oc = origin.sub(center);
    let dy = dir.y;
    let dxz = Vec3::new(dir.x, 0.0, dir.z);
    let oxz = Vec3::new(oc.x, 0.0, oc.z);
    let a = dxz.dot(dxz);
    let mut best: Option<f32> = None;
    if a > 1e-10 {
        // 圆柱侧面：|oxz + t*dxz| = r
        let b = oxz.dot(dxz);
        let c = oxz.dot(oxz) - radius * radius;
        let disc = b * b - a * c;
        if disc >= 0.0 {
            let s = disc.sqrt();
            for t in [(-b - s) / a, (-b + s) / a] {
                if t <= 1e-4 {
                    continue;
                }
                let y = oc.y + t * dy;
                if y.abs() <= half_height && best.is_none_or(|bt| t < bt) {
                    best = Some(t);
                }
            }
        }
    }
    // 两端半球帽。
    for sign in [-1.0f32, 1.0] {
        let cap = Vec3::new(center.x, center.y + sign * half_height, center.z);
        if let Some(t) = ray_sphere_raw(origin, dir, cap, radius)
            && best.is_none_or(|bt| t < bt)
        {
            best = Some(t);
        }
    }
    best
}

/// 不单位化 dir 的球测试（ray_capsule 内部已单位化）。
fn ray_sphere_raw(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let oc = origin.sub(center);
    let b = oc.dot(dir);
    let c = oc.dot(oc) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let t0 = -b - s;
    let t1 = -b + s;
    if t0 > 1e-4 {
        Some(t0)
    } else if t1 > 1e-4 {
        Some(t1)
    } else {
        None
    }
}

/// 点到射线距离（抛射物宽体判定用）。
pub fn ray_point_dist(origin: Vec3, dir: Vec3, point: Vec3) -> f32 {
    let rel = point.sub(origin);
    let t = rel.dot(dir).max(0.0);
    rel.sub(dir.scale(t)).length()
}

/// 轴对齐盒子。目前训练不用，留给以后的立方体目标。
pub fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let dir = dir.normalized();
    let inv = Vec3::new(
        if dir.x.abs() < 1e-8 {
            f32::MAX
        } else {
            1.0 / dir.x
        },
        if dir.y.abs() < 1e-8 {
            f32::MAX
        } else {
            1.0 / dir.y
        },
        if dir.z.abs() < 1e-8 {
            f32::MAX
        } else {
            1.0 / dir.z
        },
    );
    let mut tmin = (min.x - origin.x) * inv.x;
    let mut tmax = (max.x - origin.x) * inv.x;
    if tmin > tmax {
        std::mem::swap(&mut tmin, &mut tmax);
    }
    let mut tymin = (min.y - origin.y) * inv.y;
    let mut tymax = (max.y - origin.y) * inv.y;
    if tymin > tymax {
        std::mem::swap(&mut tymin, &mut tymax);
    }
    if tmin > tymax || tymin > tmax {
        return None;
    }
    if tymin > tmin {
        tmin = tymin;
    }
    if tymax < tmax {
        tmax = tymax;
    }
    let mut tzmin = (min.z - origin.z) * inv.z;
    let mut tzmax = (max.z - origin.z) * inv.z;
    if tzmin > tzmax {
        std::mem::swap(&mut tzmin, &mut tzmax);
    }
    if tmin > tzmax || tzmin > tmax {
        return None;
    }
    if tzmin > tmin {
        tmin = tzmin;
    }
    if tmin > 1e-4 {
        Some(tmin)
    } else if tmax > 1e-4 {
        Some(tmax)
    } else {
        None
    }
}

/// 一次命中：目标 id + 射线参数 t。viewport 用 t 挑最近的球。
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub id: u32,
    pub t: f32,
}

/// 一堆命中里取 t 最小的那个。空迭代返回 None。
pub fn closest_hit(hits: impl IntoIterator<Item = Hit>) -> Option<Hit> {
    hits.into_iter().min_by(|a, b| a.t.total_cmp(&b.t))
}

#[cfg(test)]
mod tests {
    use super::{ray_aabb, ray_sphere};
    use crate::vec3::Vec3;

    #[test]
    fn ray_sphere_should_hit_centered_target() {
        let origin = Vec3::new(0.0, 0.0, 0.0);
        let dir = Vec3::new(0.0, 0.0, -1.0);
        let center = Vec3::new(0.0, 0.0, -10.0);
        let t = ray_sphere(origin, dir, center, 0.25).unwrap();
        assert!((t - 9.75).abs() < 1e-4);
    }

    #[test]
    fn ray_sphere_should_miss_when_offset() {
        let origin = Vec3::new(0.0, 0.0, 0.0);
        let dir = Vec3::new(0.0, 0.0, -1.0);
        let center = Vec3::new(2.0, 0.0, -10.0);
        assert!(ray_sphere(origin, dir, center, 0.25).is_none());
    }

    #[test]
    fn ray_aabb_should_hit_cube_in_front() {
        let origin = Vec3::new(0.0, 0.0, 0.0);
        let dir = Vec3::new(0.0, 0.0, -1.0);
        let min = Vec3::new(-0.5, -0.5, -10.5);
        let max = Vec3::new(0.5, 0.5, -9.5);
        let t = ray_aabb(origin, dir, min, max).unwrap();
        assert!((t - 9.5).abs() < 1e-4);
    }
}
