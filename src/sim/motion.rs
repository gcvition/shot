//! 目标运动模型（M1 主体）。**纯函数、确定性**：不给 Bevy 依赖，
//! 每个目标带自己派生的 `ChaCha8Rng`，所以目标 A 的随机序列
//! 不会因为目标 B 先死而改变。
//!
//! 统一接口（计划 §7.1）：
//! [`Motion::step`] 每 fixed tick 推进一次 [`KinState`]。
//! 渲染端把 `prev`/`curr` 按 `overstep_fraction` 插值，命中判定用插值位置。

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::vec3::Vec3 as V3;

/// 轴对齐包围盒（米）。
#[derive(Clone, Copy, Debug, Default)]
pub struct Aabb {
    pub min: V3,
    pub max: V3,
}

/// 一个目标的运动学状态。
#[derive(Clone, Copy, Debug, Default)]
pub struct KinState {
    /// 中心位置（米）。
    pub pos: V3,
    pub vel: V3,
    pub on_ground: bool,
}

/// 模拟上下文：运动模型需要的外部世界信息。
#[derive(Clone, Copy, Debug)]
pub struct MotionCtx {
    /// 玩家眼睛位置。
    pub player_eye: V3,
    /// 允许活动的范围（撞边反弹 / 距离带）。
    pub bounds: Aabb,
    /// 本 tick 的模拟时间（秒，局内从 0 起）。
    pub time: f32,
}

impl Default for MotionCtx {
    fn default() -> Self {
        Self {
            player_eye: V3::new(0.0, 1.62, 0.0),
            bounds: Aabb::default(),
            time: 0.0,
        }
    }
}

/// 出生点确定时每个目标的运动实例（配置 + 运行时状态打包）。
///
/// 所有带 `(f32, f32)` 的参数都是「区间」——初始化时用目标自己的
/// `ChaCha8Rng` 采一个具体值，同种子下结果固定。
#[derive(Clone, Debug)]
pub enum Motion {
    /// 不动。
    Static,
    /// 匀速直线，撞 bounds 反弹。
    Linear {
        speed: f32,
        dir: V3,
    },
    /// 左右横移：变向间隔随机，加减速有过程（不是瞬间反向）。
    /// `dir` 当前横移方向（±1，`axis` 轴上）。
    Strafe {
        speed: f32,
        interval: (f32, f32),
        accel: f32,
        axis: V3,
        dir: f32,
        next_flip: f32,
        t: f32,
    },
    /// 正弦摆动（可两轴叠加）。
    Sine {
        anchor: V3,
        amp: V3,
        freq: V3,
        phase: V3,
    },
    /// 绕点转圈。`center: None` = 绕玩家眼睛。
    Orbit {
        center: Option<V3>,
        radius: f32,
        angular_speed: f32,
        angle: f32,
        y_offset: f32,
    },
    /// Ornstein-Uhlenbeck 速度过程：平滑的随机漂移。
    SmoothRandom { max_speed: f32, tau: f32 },
    /// KovaaK Dodge Profile 等价物：横移 + 前后 + 跳 + 受击反应变向。
    Dodge(Box<DodgeState>),
    /// 垂直起落，可叠加在别的运动上。
    Jumper {
        jump_speed: f32,
        gravity: f32,
        interval: (f32, f32),
        ground_y: f32,
        next_jump: f32,
        t: f32,
    },
    /// 瞬移：每隔 interval 随机跳到 dist 范围内的一点。
    Blink {
        interval: (f32, f32),
        dist: (f32, f32),
        next_blink: f32,
        t: f32,
    },
    /// 沿航点走。
    Path {
        points: Vec<V3>,
        speed: f32,
        closed: bool,
        index: usize,
    },
    /// 与玩家保持距离带（远了靠近、近了拉远）。
    Distance { band: (f32, f32), speed: f32 },
    /// 按顺序叠加子模型（各写各的分量）。
    Composite(Vec<Motion>),
}

/// `Dodge` 的运行时状态（大，装箱）。
#[derive(Clone, Debug)]
pub struct DodgeState {
    pub speed: f32,
    pub accel: f32,
    pub gravity: f32,
    pub jump_speed: f32,
    /// 左右 / 前后变向间隔区间（秒）。
    pub lr_interval: (f32, f32),
    pub fb_interval: (f32, f32),
    pub jump_interval: (f32, f32),
    /// 与玩家的距离带。
    pub dist_band: (f32, f32),
    /// 被打后变向的概率（0..1）和反应延迟区间。
    pub react_chance: f32,
    pub react_delay: (f32, f32),
    pub react_cooldown: f32,
    // --- 运行时 ---
    pub lr_dir: f32,
    pub fb_dir: f32,
    pub next_lr: f32,
    pub next_fb: f32,
    pub next_jump: f32,
    pub react_at: f32,
    pub react_cooled_at: f32,
    pub ground_y: f32,
    pub t: f32,
}

impl Default for DodgeState {
    fn default() -> Self {
        Self {
            speed: 3.0,
            accel: 30.0,
            gravity: 20.0,
            jump_speed: 6.0,
            lr_interval: (0.5, 1.5),
            fb_interval: (0.8, 2.0),
            jump_interval: (1.0, 3.0),
            dist_band: (6.0, 16.0),
            react_chance: 0.5,
            react_delay: (0.1, 0.35),
            react_cooldown: 0.3,
            lr_dir: 1.0,
            fb_dir: 1.0,
            // 默认给非零计时器：避免第一 tick 就「到点变向」。
            // `to_motion` 里会按区间重新采样覆盖。
            next_lr: 1.0,
            next_fb: 1.5,
            next_jump: 2.0,
            react_at: f32::MAX,
            react_cooled_at: 0.0,
            ground_y: 0.0,
            t: 0.0,
        }
    }
}

fn sample_range(rng: &mut ChaCha8Rng, (lo, hi): (f32, f32)) -> f32 {
    if hi <= lo {
        lo
    } else {
        lo + rng.random::<f32>() * (hi - lo)
    }
}

/// Box-Muller，从均匀分布出标准正态（确定性：只用 rng）。
fn gaussian(rng: &mut ChaCha8Rng) -> f32 {
    let u1 = rng.random::<f32>().max(1e-9);
    let u2 = rng.random::<f32>();
    (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
}

fn clamp_to_bounds(pos: V3, bounds: &Aabb) -> V3 {
    V3::new(
        pos.x.clamp(bounds.min.x, bounds.max.x),
        pos.y.clamp(bounds.min.y, bounds.max.y),
        pos.z.clamp(bounds.min.z, bounds.max.z),
    )
}

/// 撞边反弹：出了哪条边就把那条边的速度分量反过来。
fn bounce(pos: &mut V3, vel: &mut V3, bounds: &Aabb) {
    if pos.x < bounds.min.x || pos.x > bounds.max.x {
        pos.x = pos.x.clamp(bounds.min.x, bounds.max.x);
        vel.x = -vel.x;
    }
    if pos.y < bounds.min.y || pos.y > bounds.max.y {
        pos.y = pos.y.clamp(bounds.min.y, bounds.max.y);
        vel.y = -vel.y;
    }
    if pos.z < bounds.min.z || pos.z > bounds.max.z {
        pos.z = pos.z.clamp(bounds.min.z, bounds.max.z);
        vel.z = -vel.z;
    }
}

impl Motion {
    /// 每 fixed tick 推进一步。`rng` 是该目标自己的派生 RNG。
    pub fn step(&mut self, s: &mut KinState, ctx: &MotionCtx, rng: &mut ChaCha8Rng, dt: f32) {
        match self {
            Self::Static => {}
            Self::Linear { speed, dir } => {
                s.vel = dir.scale(*speed);
                s.pos = s.pos.add(s.vel.scale(dt));
                bounce(&mut s.pos, &mut s.vel, &ctx.bounds);
                *dir = s.vel.normalized();
            }
            Self::Strafe {
                speed,
                interval,
                accel,
                axis,
                dir,
                next_flip,
                t,
            } => {
                *t += dt;
                if *t >= *next_flip {
                    *dir = -*dir;
                    *next_flip = *t + sample_range(rng, *interval);
                }
                let want = axis.scale(*dir * *speed);
                let dv = want.sub(s.vel);
                let dv_len = dv.length();
                let max_dv = *accel * dt;
                if dv_len > max_dv && dv_len > 1e-6 {
                    s.vel = s.vel.add(dv.scale(max_dv / dv_len));
                } else {
                    s.vel = want;
                }
                s.pos = s.pos.add(s.vel.scale(dt));
                if s.pos.x < ctx.bounds.min.x || s.pos.x > ctx.bounds.max.x {
                    *dir = -*dir;
                }
                s.pos = clamp_to_bounds(s.pos, &ctx.bounds);
                s.vel.y = 0.0;
            }
            Self::Sine {
                anchor,
                amp,
                freq,
                phase,
            } => {
                let t = ctx.time;
                s.pos = V3::new(
                    anchor.x + amp.x * (std::f32::consts::TAU * freq.x * t + phase.x).sin(),
                    anchor.y + amp.y * (std::f32::consts::TAU * freq.y * t + phase.y).sin(),
                    anchor.z + amp.z * (std::f32::consts::TAU * freq.z * t + phase.z).sin(),
                );
            }
            Self::Orbit {
                center,
                radius,
                angular_speed,
                angle,
                y_offset,
            } => {
                *angle += *angular_speed * dt;
                let c = center.unwrap_or(ctx.player_eye);
                s.pos = V3::new(
                    c.x + *radius * angle.cos(),
                    c.y + *y_offset,
                    c.z + *radius * angle.sin(),
                );
            }
            Self::SmoothRandom { max_speed, tau } => {
                // OU 过程：dv = -v/τ·dt + σ·√dt·N(0,1)；σ 按 max_speed 标定。
                let tau = (*tau).max(1e-3);
                let sigma = *max_speed * (2.0 / tau).sqrt();
                s.vel = s
                    .vel
                    .add(s.vel.scale(-dt / tau))
                    .add(V3::new(gaussian(rng), gaussian(rng), gaussian(rng)).scale(sigma * dt.sqrt()));
                if s.vel.length() > *max_speed {
                    s.vel = s.vel.normalized().scale(*max_speed);
                }
                s.pos = s.pos.add(s.vel.scale(dt));
                bounce(&mut s.pos, &mut s.vel, &ctx.bounds);
            }
            Self::Dodge(d) => {
                d.t += dt;
                // 受击反应到点：立刻横移变向。
                if d.t >= d.react_at {
                    d.lr_dir = -d.lr_dir;
                    d.react_at = f32::MAX;
                }
                if d.t >= d.next_lr {
                    d.lr_dir = -d.lr_dir;
                    d.next_lr = d.t + sample_range(rng, d.lr_interval);
                }
                if d.t >= d.next_fb {
                    d.fb_dir = -d.fb_dir;
                    d.next_fb = d.t + sample_range(rng, d.fb_interval);
                }
                if d.t >= d.next_jump && s.on_ground {
                    s.vel.y = d.jump_speed;
                    s.on_ground = false;
                    d.next_jump = d.t + sample_range(rng, d.jump_interval);
                }
                // 距离带修正：远了往玩家靠，近了拉开。
                let to_player = ctx.player_eye.sub(s.pos);
                let dist = to_player.length().max(1e-6);
                let fb_axis = to_player.scale(1.0 / dist);
                let mut band_sign: f32 = 0.0;
                if dist > d.dist_band.1 {
                    band_sign = 1.0;
                } else if dist < d.dist_band.0 {
                    band_sign = -1.0;
                }
                let want = V3::new(d.lr_dir * d.speed, 0.0, 0.0).add(
                    fb_axis.scale(band_sign.max(0.0) * d.speed + d.fb_dir * 0.3 * d.speed),
                );
                let dv = want.sub(V3::new(s.vel.x, 0.0, s.vel.z));
                let dv_len = dv.length();
                let max_dv = d.accel * dt;
                let horiz = if dv_len > max_dv && dv_len > 1e-6 {
                    V3::new(s.vel.x, 0.0, s.vel.z).add(dv.scale(max_dv / dv_len))
                } else {
                    want
                };
                s.vel = V3::new(horiz.x, s.vel.y, horiz.z);
                // 竖直方向：重力 + 落地。
                s.vel.y -= d.gravity * dt;
                s.pos = s.pos.add(s.vel.scale(dt));
                if s.pos.y <= d.ground_y {
                    s.pos.y = d.ground_y;
                    s.vel.y = 0.0;
                    s.on_ground = true;
                }
                s.pos = clamp_to_bounds(s.pos, &ctx.bounds);
            }
            Self::Jumper {
                jump_speed,
                gravity,
                interval,
                ground_y,
                next_jump,
                t,
            } => {
                *t += dt;
                if s.on_ground && *t >= *next_jump {
                    s.vel.y = *jump_speed;
                    s.on_ground = false;
                    *next_jump = *t + sample_range(rng, *interval);
                }
                s.vel.y -= *gravity * dt;
                s.pos.y += s.vel.y * dt;
                if s.pos.y <= *ground_y {
                    s.pos.y = *ground_y;
                    s.vel.y = 0.0;
                    s.on_ground = true;
                }
            }
            Self::Blink {
                interval,
                dist,
                next_blink,
                t,
            } => {
                *t += dt;
                if *t >= *next_blink {
                    let yaw = rng.random::<f32>() * std::f32::consts::TAU;
                    let r = sample_range(rng, *dist);
                    s.pos = clamp_to_bounds(
                        s.pos.add(V3::new(yaw.cos() * r, 0.0, yaw.sin() * r)),
                        &ctx.bounds,
                    );
                    *next_blink = *t + sample_range(rng, *interval);
                }
            }
            Self::Path {
                points,
                speed,
                closed,
                index,
            } => {
                if points.len() >= 2 && *index < points.len() {
                    let target = points[*index];
                    let to = target.sub(s.pos);
                    let dist = to.length();
                    let step = *speed * dt;
                    if dist <= step.max(1e-4) {
                        s.pos = target;
                        *index += 1;
                        if *index >= points.len() && *closed {
                            *index = 0;
                        }
                    } else {
                        s.pos = s.pos.add(to.scale(step / dist));
                    }
                }
            }
            Self::Distance { band, speed } => {
                let to_player = ctx.player_eye.sub(s.pos);
                let dist = to_player.length().max(1e-6);
                let axis = to_player.scale(1.0 / dist);
                let sign = if dist > band.1 {
                    1.0
                } else if dist < band.0 {
                    -1.0
                } else {
                    0.0
                };
                s.vel = axis.scale(sign * *speed);
                s.pos = s.pos.add(s.vel.scale(dt));
            }
            Self::Composite(parts) => {
                for part in parts {
                    part.step(s, ctx, rng, dt);
                }
            }
        }
    }

    /// 被打时的反应（KovaaK `DamageReaction*`）。只对 `Dodge` 有效。
    pub fn on_hit(&mut self, t: f32, rng: &mut ChaCha8Rng) {
        if let Self::Dodge(d) = self
            && t >= d.react_cooled_at
            && rng.random::<f32>() < d.react_chance
        {
            d.react_at = d.t + sample_range(rng, d.react_delay);
            d.react_cooled_at = t + d.react_cooldown;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;

    fn ctx() -> MotionCtx {
        MotionCtx {
            player_eye: V3::new(0.0, 1.62, 0.0),
            bounds: Aabb {
                min: V3::new(-10.0, 0.0, -20.0),
                max: V3::new(10.0, 5.0, 0.0),
            },
            time: 0.0,
        }
    }

    fn rng() -> ChaCha8Rng {
        ChaCha8Rng::seed_from_u64(7)
    }

    #[test]
    fn linear_should_bounce_off_bounds() {
        let mut m = Motion::Linear {
            speed: 5.0,
            dir: V3::new(1.0, 0.0, 0.0),
        };
        let mut s = KinState {
            pos: V3::new(9.9, 1.0, -5.0),
            ..Default::default()
        };
        let mut r = rng();
        for _ in 0..100 {
            m.step(&mut s, &ctx(), &mut r, 1.0 / 240.0);
        }
        assert!(s.pos.x <= 10.0 && s.pos.x >= -10.0, "x={}", s.pos.x);
        assert!(s.vel.x < 0.0, "应该反弹，vel.x={}", s.vel.x);
    }

    #[test]
    fn strafe_should_eventually_reverse() {
        let mut m = Motion::Strafe {
            speed: 3.0,
            interval: (0.1, 0.2),
            accel: 30.0,
            axis: V3::new(1.0, 0.0, 0.0),
            dir: 1.0,
            next_flip: 0.1,
            t: 0.0,
        };
        let mut s = KinState::default();
        let mut r = rng();
        let mut min_vx = f32::MAX;
        let mut max_vx = f32::MIN;
        for _ in 0..240 * 3 {
            m.step(&mut s, &ctx(), &mut r, 1.0 / 240.0);
            min_vx = min_vx.min(s.vel.x);
            max_vx = max_vx.max(s.vel.x);
        }
        assert!(min_vx < -1.0, "应该向左走过 {min_vx}");
        assert!(max_vx > 1.0, "应该向右走过 {max_vx}");
    }

    #[test]
    fn smooth_random_should_stay_bounded_and_smooth() {
        let mut m = Motion::SmoothRandom {
            max_speed: 4.0,
            tau: 0.3,
        };
        let mut s = KinState::default();
        let mut r = rng();
        let mut prev = s.vel;
        let mut max_dv = 0.0f32;
        for _ in 0..240 {
            m.step(&mut s, &ctx(), &mut r, 1.0 / 240.0);
            max_dv = max_dv.max(s.vel.sub(prev).length());
            prev = s.vel;
            assert!(s.vel.length() <= 4.0 + 1e-4);
        }
        assert!(max_dv < 4.0, "OU 速度应该平滑，max_dv={max_dv}");
    }

    #[test]
    fn orbit_should_keep_radius() {
        let mut m = Motion::Orbit {
            center: Some(V3::new(0.0, 1.0, -5.0)),
            radius: 2.0,
            angular_speed: 1.0,
            angle: 0.0,
            y_offset: 0.5,
        };
        let mut s = KinState::default();
        let mut r = rng();
        for _ in 0..240 {
            m.step(&mut s, &ctx(), &mut r, 1.0 / 240.0);
        }
        let d = (s.pos.x * s.pos.x + (s.pos.z + 5.0) * (s.pos.z + 5.0)).sqrt();
        assert!((d - 2.0).abs() < 1e-4, "d={d}");
        assert!((s.pos.y - 1.5).abs() < 1e-4);
    }

    #[test]
    fn dodge_hit_reaction_should_flip_direction() {
        let mut m = Motion::Dodge(Box::new(DodgeState {
            react_chance: 1.0,
            react_delay: (0.05, 0.05),
            ..Default::default()
        }));
        let mut s = KinState {
            pos: V3::new(0.0, 0.0, -8.0),
            on_ground: true,
            ..Default::default()
        };
        let mut r = rng();
        let dir_before = match &m {
            Motion::Dodge(d) => d.lr_dir,
            _ => unreachable!(),
        };
        m.on_hit(0.0, &mut r);
        // 模拟跑到反应延迟之后。
        for _ in 0..60 {
            m.step(&mut s, &ctx(), &mut r, 1.0 / 240.0);
        }
        match &m {
            Motion::Dodge(d) => assert_eq!(d.lr_dir, -dir_before),
            _ => unreachable!(),
        }
    }

    #[test]
    fn same_seed_should_give_same_path() {
        let run = || {
            let mut m = Motion::SmoothRandom {
                max_speed: 4.0,
                tau: 0.3,
            };
            let mut s = KinState::default();
            let mut r = ChaCha8Rng::seed_from_u64(42);
            for _ in 0..480 {
                m.step(&mut s, &ctx(), &mut r, 1.0 / 240.0);
            }
            s.pos
        };
        assert_eq!(run(), run());
    }
}
