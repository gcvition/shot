//! 世界坐标 → 渲染分辨率上的像素。回放轨迹用，不走 GPU。
//!
//! 和 Bevy 相机一致：yaw/pitch 用 [`crate::scenario::look_direction`]，垂直 FOV + aspect
//! 做透视。点在相机后面（depth 太小）返回 None。

use crate::scenario::look_direction;
use crate::vec3::Vec3;

/// 投影到渲染缓冲像素。返回 `(sx, sy, depth)`，原点在左上，Y 向下。
pub fn project_world(
    world: Vec3,
    eye: Vec3,
    yaw_deg: f32,
    pitch_deg: f32,
    vfov_rad: f32,
    aspect: f32,
    render_w: f32,
    render_h: f32,
) -> Option<(f32, f32, f32)> {
    let forward = look_direction(yaw_deg, pitch_deg);
    let yaw = yaw_deg.to_radians();
    let right = Vec3::new(yaw.cos(), 0.0, yaw.sin());
    let up = Vec3::new(
        right.y * forward.z - right.z * forward.y,
        right.z * forward.x - right.x * forward.z,
        right.x * forward.y - right.y * forward.x,
    );
    let to = world.sub(eye);
    let depth = to.dot(forward);
    if depth <= 0.05 {
        return None;
    }
    let x = to.dot(right);
    let y = to.dot(up);
    let tan_half = (vfov_rad * 0.5).tan();
    let ndc_x = x / (depth * tan_half * aspect);
    let ndc_y = y / (depth * tan_half);
    let sx = (ndc_x * 0.5 + 0.5) * render_w;
    let sy = (1.0 - (ndc_y * 0.5 + 0.5)) * render_h;
    Some((sx, sy, depth))
}

#[cfg(test)]
mod tests {
    use super::project_world;
    use crate::scenario::PLAYER_EYE;
    use crate::vec3::Vec3;

    #[test]
    fn point_on_look_axis_should_project_to_center() {
        let world = Vec3::new(0.0, PLAYER_EYE.y, -10.0);
        let (x, y, _) = project_world(
            world,
            PLAYER_EYE,
            0.0,
            0.0,
            90.0_f32.to_radians(),
            16.0 / 9.0,
            1920.0,
            1080.0,
        )
        .unwrap();
        assert!((x - 960.0).abs() < 2.0);
        assert!((y - 540.0).abs() < 2.0);
    }
}
