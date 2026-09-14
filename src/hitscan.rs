use crate::vec3::Vec3;

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

pub fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let dir = dir.normalized();
    let inv = Vec3::new(
        if dir.x.abs() < 1e-8 {
            f32::INFINITY
        } else {
            1.0 / dir.x
        },
        if dir.y.abs() < 1e-8 {
            f32::INFINITY
        } else {
            1.0 / dir.y
        },
        if dir.z.abs() < 1e-8 {
            f32::INFINITY
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

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub id: u32,
    pub t: f32,
}

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
