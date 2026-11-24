//! M4：玩家移动模拟。纯函数、无 Bevy 依赖，fixed tick 确定性驱动。
//!
//! - [`params`]：三套移动预设（Tactical / Arena / BattleRoyale）。
//! - [`step`]：一帧移动（摩擦 + 加速 + 跳跃 + 重力 + 地面钳制）。
//! - [`resolve_blocks`]：把玩家从障碍 AABB 里推出去（水平推出 + 站上顶面）。

use crate::sce::{Block, MovementPreset};
use crate::vec3::Vec3;

/// 一套移动参数。
#[derive(Clone, Copy, Debug)]
pub struct MoveParams {
    /// 地面加速度 m/s²。
    pub accel: f32,
    /// 地面摩擦（每秒衰减速率）。
    pub friction: f32,
    /// 水平速度上限 m/s。
    pub max_speed: f32,
    /// 空中加速度（Arena 有空中控制）。
    pub air_accel: f32,
    /// 起跳初速 m/s。
    pub jump_vel: f32,
    /// 重力 m/s²。
    pub gravity: f32,
    /// 蹲下速度倍率。
    pub crouch_mult: f32,
}

/// 按预设取移动参数。Static 也返回一套（调用方应直接跳过）。
pub fn params(preset: MovementPreset) -> MoveParams {
    match preset {
        MovementPreset::Static | MovementPreset::Tactical => MoveParams {
            accel: 50.0,
            friction: 9.0,
            max_speed: 4.3,
            air_accel: 8.0,
            jump_vel: 4.6,
            gravity: 14.0,
            crouch_mult: 0.45,
        },
        MovementPreset::Arena => MoveParams {
            accel: 80.0,
            friction: 3.5,
            max_speed: 6.5,
            air_accel: 40.0,
            jump_vel: 5.6,
            gravity: 16.0,
            crouch_mult: 0.5,
        },
        MovementPreset::BattleRoyale => MoveParams {
            accel: 60.0,
            friction: 7.0,
            max_speed: 5.2,
            air_accel: 15.0,
            jump_vel: 5.0,
            gravity: 15.0,
            crouch_mult: 0.5,
        },
    }
}

/// 玩家碰撞半宽（水平半径）和身高。
pub const PLAYER_HALF: f32 = 0.32;
pub const PLAYER_HEIGHT: f32 = 1.75;

/// 一个 fixed tick 的移动。`wish` = 水平期望方向（未单位化也行，内部归一）。
/// 返回 (pos, vel, on_ground)。
pub fn step(
    pos: Vec3,
    vel: Vec3,
    wish: Vec3,
    jump: bool,
    crouch: bool,
    on_ground: bool,
    dt: f32,
    p: &MoveParams,
) -> (Vec3, Vec3, bool) {
    let mut v = vel;
    let mut grounded = on_ground;
    let mut wish = Vec3::new(wish.x, 0.0, wish.z);
    let wl = wish.length();
    if wl > 1e-6 {
        wish = wish.scale(1.0 / wl);
    }
    let cap = p.max_speed * if crouch { p.crouch_mult } else { 1.0 };

    if grounded {
        // 摩擦：水平速度按速率衰减（停得住为止）。
        let hs = Vec3::new(v.x, 0.0, v.z).length();
        if hs > 1e-4 {
            let drop = hs * p.friction * dt;
            let ns = (hs - drop).max(0.0) / hs;
            v.x *= ns;
            v.z *= ns;
        }
        // 地面加速：朝 wish 方向加，不超过 cap（经典 Quake 式沿投影加速）。
        let cur = v.x * wish.x + v.z * wish.z;
        let add = (cap - cur).min(p.accel * dt).max(0.0);
        if wl > 1e-6 {
            v.x += wish.x * add;
            v.z += wish.z * add;
        }
        if jump {
            v.y = p.jump_vel;
            grounded = false;
        }
    } else {
        // 空中：弱加速，无摩擦。沿 wish 投影封顶 cap*1.8（Quake air control）。
        if wl > 1e-6 && p.air_accel > 0.0 {
            let cur = v.x * wish.x + v.z * wish.z;
            let add = (cap * 1.8 - cur).min(p.air_accel * dt).max(0.0);
            v.x += wish.x * add;
            v.z += wish.z * add;
        }
    }
    v.y -= p.gravity * dt;
    let mut np = pos.add(v.scale(dt));
    if np.y <= 0.0 {
        np.y = 0.0;
        if v.y < 0.0 {
            v.y = 0.0;
        }
        grounded = true;
    }
    (np, v, grounded)
}

/// 把玩家胶囊（近似 AABB：半径 r、高 h）从 blocks 里推出。
/// 脚在 block 顶面附近（±step）算站上，不做水平推出。
pub fn resolve_blocks(pos: Vec3, r: f32, h: f32, blocks: &[Block], step: f32) -> (Vec3, bool) {
    let mut p = pos;
    let mut landed = false;
    for b in blocks {
        let feet = p.y;
        let px1 = p.x - r;
        let px2 = p.x + r;
        let pz1 = p.z - r;
        let pz2 = p.z - r;
        if px2 <= b.min.x || px1 >= b.max.x || pz2 <= b.min.z || pz1 >= b.max.z {
            continue;
        }
        // 脚落在顶面 ±step 带内 → 站上去（矮台阶可走、跳上高台能落地）。
        if feet >= b.max.y - step && feet <= b.max.y {
            p.y = b.max.y;
            landed = true;
            continue;
        }
        if feet >= b.max.y || feet + h <= b.min.y {
            continue;
        }
        // 最小推出轴（水平）。
        let push_x1 = px2 - b.min.x; // 往 -x 推
        let push_x2 = b.max.x - px1; // 往 +x 推
        let push_z1 = pz2 - b.min.z;
        let push_z2 = b.max.z - pz1;
        let min = push_x1.min(push_x2).min(push_z1).min(push_z2);
        if min == push_x1 {
            p.x = b.min.x - r;
        } else if min == push_x2 {
            p.x = b.max.x + r;
        } else if min == push_z1 {
            p.z = b.min.z - r;
        } else {
            p.z = b.max.z + r;
        }
    }
    (p, landed)
}

/// 把脚点钳制在房间 AABB 内（bounds 缺省 = 默认白房）。
pub fn clamp_bounds(pos: Vec3, r: f32, bounds: Option<(Vec3, Vec3)>) -> Vec3 {
    let (min, max) = bounds.unwrap_or((
        Vec3::new(-9.0, 0.0, -8.0),
        Vec3::new(9.0, 6.0, 1.0),
    ));
    Vec3::new(
        pos.x.clamp(min.x + r, max.x - r),
        pos.y.max(0.0),
        pos.z.clamp(min.z + r, max.z - r),
    )
}

/// 期望方向：WASD 本地向量按 yaw（度）旋转到世界系。
pub fn wish_dir(forward: f32, right: f32, yaw_deg: f32) -> Vec3 {
    let (sy, cy) = yaw_deg.to_radians().sin_cos();
    // look_direction(yaw,0) = (sin(yaw),0,-cos(yaw)) 为前；右 = (cos,0,sin)。
    Vec3::new(sy * forward + cy * right, 0.0, -cy * forward + sy * right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accel_should_reach_cap_and_stop_on_release() {
        let p = params(MovementPreset::Tactical);
        let mut pos = Vec3::ZERO;
        let mut v = Vec3::ZERO;
        let mut g = true;
        for _ in 0..240 {
            // 1s @240Hz 全速前进。
            (pos, v, g) = step(pos, v, Vec3::new(0.0, 0.0, -1.0), false, false, g, 1.0 / 240.0, &p);
        }
        let hs = Vec3::new(v.x, 0.0, v.z).length();
        assert!((hs - p.max_speed).abs() < 0.2, "应接近上限 {hs}");
        for _ in 0..240 {
            (pos, v, g) = step(pos, v, Vec3::ZERO, false, false, g, 1.0 / 240.0, &p);
        }
        let hs2 = Vec3::new(v.x, 0.0, v.z).length();
        assert!(hs2 < 0.01, "1s 松开后应停 {hs2}");
    }

    #[test]
    fn resolve_should_push_out_of_block() {
        let b = vec![Block {
            min: Vec3::new(-1.0, 0.0, -1.0),
            max: Vec3::new(1.0, 2.0, 1.0),
        }];
        let (p, landed) = resolve_blocks(Vec3::new(0.8, 0.0, 0.0), 0.32, 1.75, &b, 0.4);
        assert!((p.x - (1.0 + 0.32)).abs() < 1e-4, "应往 +x 推出 {p:?}");
        assert!(!landed);
    }

    #[test]
    fn standing_on_top_should_not_push() {
        let b = vec![Block {
            min: Vec3::new(-1.0, 0.0, -1.0),
            max: Vec3::new(1.0, 1.0, 1.0),
        }];
        let (p, landed) = resolve_blocks(Vec3::new(0.0, 1.0, 0.0), 0.32, 1.75, &b, 0.4);
        assert_eq!(p.x, 0.0);
        // 脚底正好在顶面上沿 → 站上去。
        assert!(landed);
    }
}
