//! 自瞄的 Bevy 接线：每帧读 `RenderPos` 目标 → `Humanizer` → 合成输入。
//!
//! 注入点选在**输入消息层**而非直接改 `Look`/开火，目的是让自瞄走和人
//! 完全相同的管线：`MouseMotion` → `apply_mouse_look`（含灵敏度/ADS/
//! 后坐抵消/轨迹记录），`ButtonInput` → `fire_hitscan`（含 RPM/弹匣/
//! 散布/计分）。这样测的是"算法像不像人"，不是"算法能不能绕过游戏"。
//!
//! 激活语义（对齐 Rain `HotkeyId::Aim` 按住生效）：`launch.aimbot`
//! （设置开关/`--aimbot`/`SHOT_AIMBOT=1`）武装本系统，运行时按住
//! `settings.aimbot_key`（默认鼠标右键）才工作；松开即 `StopAiming`
//! 清全部状态并松开扳机。

use std::collections::VecDeque;

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;

use crate::settings::{AimAlgo, AimbotButton, AimbotPart};
use crate::viewport::{
    AimbotFovRing, AppPathsRes, DegPerCount, FireGate, Hitbox, Hitboxes, Look, MatchClock,
    PlayerBody, RenderPos, ScoreState, SettingsRes, SpawnDirector, Target, Team, ViewportLaunch,
    WeaponState, WorldCam,
};

use super::algo::ConstAim;
use super::humanizer::{AimTune, Humanizer, pick_target};
use super::log::AimbotLog;

/// 目标速度估计的位置历史窗口（秒）。窗口本身是感知滞后的特性
/// （aimbot.md §6.2：速度突变延迟反映到提前量才像人）。
const VEL_HIST_SECS: f64 = 0.12;

/// 自瞄控制器。激活状态由 `launch.aimbot` + 持键在系统内判定，
/// 这里只存运行态。
#[derive(Resource, Default)]
pub struct AimbotCtl {
    pub tune: AimTune,
    hum: Humanizer,
    /// 匀速系步进器（AFG/NEXO/Nep 三模式；Rain 走 `hum`）。
    con: ConstAim,
    /// 左键按住的截止 sim 秒；None = 当前未按。
    holding_until: Option<f64>,
    /// 按下时刻（sim 秒）——按住上限与时间回跳的保险。
    hold_pressed_at: f64,
    /// 候选缓冲（每帧复用，不逐帧分配）。
    cands: Vec<(u64, f64, f64)>,
    /// 锁定目标的 (t, RenderPos) 历史——位置差分估计速度。
    hist: VecDeque<(f64, crate::vec3::Vec3)>,
    /// 累计输出位移（counts），调试/HUD 用。
    pub moved: (i64, i64),
    /// 诊断日志（`cache/logs/aimbot-*.log`；headless 无 AppPathsRes 不写）。
    pub log: AimbotLog,
}

impl AimbotButton {
    /// 映射到 bevy 鼠标键（`Left` 不可绑——和扳机冲突）。
    pub(crate) fn to_mouse_button(self) -> MouseButton {
        match self {
            Self::Right => MouseButton::Right,
            Self::Middle => MouseButton::Middle,
            Self::Back => MouseButton::Back,
            Self::Forward => MouseButton::Forward,
        }
    }
}

/// 归一化角度差到 (-180, 180]。
fn wrap180(mut d: f32) -> f32 {
    while d > 180.0 {
        d -= 360.0;
    }
    while d <= -180.0 {
        d += 360.0;
    }
    d
}

/// 按住上限（sim 秒）：超过即强制松键，防 sim_time 异常时按钮卡死。
const MAX_HOLD_SECS: f64 = 1.0;

/// 自瞄驱动：`PlaySet::Aimbot`（Camera 之后、Fire 之前）。
///
/// 误差用角度差→counts（屏外/背后目标也有正确方向）；起算点与开火
/// 射线严格同源：`body.eye()` + `look + acc_*`（`fire_hitscan` 口径，
/// `punch_*` 只是显示偏移）；ADS 灵敏度变化镜像 `apply_mouse_look`
/// 的 `deg_eff` 计算。命中容差 w = 目标最大命中半径的角度/counts，
/// 对应文档 pick_target 三元组的 W（单点部位退化）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn aimbot_drive(
    mut ctl: ResMut<AimbotCtl>,
    launch: Res<ViewportLaunch>,
    settings: Res<SettingsRes>,
    look: Res<Look>,
    wstate: Res<WeaponState>,
    body: Res<PlayerBody>,
    deg: Res<DegPerCount>,
    gate: Res<FireGate>,
    clock: Res<MatchClock>,
    director: Res<SpawnDirector>,
    time: Res<Time>,
    score: Res<ScoreState>,
    paths: Option<Res<AppPathsRes>>,
    targets: Query<(&Target, &RenderPos, &Hitboxes)>,
    mut motion: MessageWriter<MouseMotion>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
) {
    let t = director.sim_time as f64;
    // 松键判定先于一切：开关关闭/时间回跳/超上限都不能让按钮卡住。
    if ctl.holding_until.is_some_and(|u| {
        t >= u || t < ctl.hold_pressed_at || t - ctl.hold_pressed_at > MAX_HOLD_SECS
    }) {
        mouse.release(MouseButton::Left);
        ctl.holding_until = None;
    }
    // 激活 = 功能武装 && 持键（aimbot.md HotkeyId::Aim 按住生效）。
    // 倒计时不激活：sim_time 冻结期 now 恒 0，状态机会卡死在 React。
    let active = launch.aimbot
        && clock.countdown <= 0.0
        && mouse.pressed(settings.0.aimbot_key.to_mouse_button());
    if !active || clock.finished {
        if ctl.hum.locked.is_some() {
            ctl.hum.stop(); // 松键/结束：StopAiming 清全部内部状态
            ctl.con.reset();
            ctl.hist.clear();
            ctl.log.line(&format!(
                "UNLOCK t={t:.3} why={}",
                if clock.finished { "match_end" } else { "key_released" }
            ));
            ctl.log.flush();
        }
        if clock.finished && ctl.log.active() {
            let frames = ctl.log.frames;
            ctl.log.line(&format!(
                "END t={t:.3} hits={} misses={} frames={frames}",
                score.hits, score.misses
            ));
            ctl.log.flush();
        }
        return;
    }
    if !(deg.0.is_finite() && deg.0 > 0.0) {
        return;
    }

    // 诊断日志：首个激活帧打开（每会话一份；无 AppPathsRes 的
    // headless 环境静默跳过）。
    if !ctl.log.active()
        && let Some(paths) = paths.as_ref()
    {
        ctl.log.ensure_open(
            &paths.0.cache.join("logs"),
            &launch.scenario.id,
            &launch.session_id,
        );
        let s = &settings.0;
        ctl.log.line(&format!(
            "START t={t:.3} scen={} sess={} key={:?} algo={:?} fov={:.1} spd={:.2}x/{:.2}y part={:?} deg={:.6}",
            launch.scenario.id,
            launch.session_id.get(..8).unwrap_or(&launch.session_id),
            s.aimbot_key,
            s.aimbot_algo,
            s.aimbot_fov_deg,
            s.aimbot_speed_x,
            s.aimbot_speed_y,
            s.aimbot_part,
            deg.0
        ));
    }
    // ScoreState 差分 → HIT/MISS 事件（命中在 fire_hitscan 里记，
    // 这里只看计数变化，不用侵入判定管线）。
    if score.hits > ctl.log.hits {
        ctl.log
            .line(&format!("HIT t={t:.3} hits={} misses={}", score.hits, score.misses));
    }
    if score.misses > ctl.log.misses {
        ctl.log
            .line(&format!("MISS t={t:.3} hits={} misses={}", score.hits, score.misses));
    }
    ctl.log.hits = score.hits;
    ctl.log.misses = score.misses;

    // ADS 镜像：右键按住且武器有 ads → 灵敏度乘 sens_mult
    // （与 apply_mouse_look 的 deg_eff 同一公式，否则绑右键时误差换算错）。
    let deg_eff = if mouse.pressed(MouseButton::Right) {
        launch
            .scenario
            .weapon
            .ads
            .map(|a| deg.0 * f64::from(a.sens_mult))
            .unwrap_or(deg.0)
    } else {
        deg.0
    };

    // 有效朝向与开火射线同源：look + 累积后坐（acc），不是显示偏移 punch。
    let cur_yaw = look.yaw + wstate.acc_yaw;
    let cur_pitch = (look.pitch + wstate.acc_pitch).clamp(-89.0, 89.0);
    let eye = body.eye();

    // 锁定目标的速度估计（位置历史差分，窗口含感知滞后）。
    // 上限 25 m/s：Blink/瞬移目标的差分会炸出离谱速度，不能让提前量
    // 把瞄准点甩到屏幕外（"天旋地转"的来源之一）。
    let mut tune = ctl.tune;
    tune.spd_mul_x = f64::from(settings.0.aimbot_speed_x);
    tune.spd_mul_y = f64::from(settings.0.aimbot_speed_y);
    let algo = settings.0.aimbot_algo;
    let vel = cap_vel(estimate_vel(&ctl.hist), 25.0);
    // 索敌半径：设置里是度数，换成 counts 参与候选筛选（与误差同单位）。
    let fov_counts = (f64::from(settings.0.aimbot_fov_deg) / deg_eff).max(10.0);

    // 候选：(id, dx_counts, dy_counts)——全部按所选部位的当前位置算
    // （无提前量），锁定目标的 intercept 提前量在选定后单独加到驱动
    // 误差上。部位偏移 = 命中体偏移（人形头 +HEAD_Y，球 0）。
    let part = settings.0.aimbot_part;
    ctl.cands.clear();
    for (tgt, rp, hb) in targets.iter() {
        if tgt.team == Team::Neutral {
            continue; // Go/No-Go 的"别打"目标
        }
        let (off, _r) = aim_part(hb, part);
        let d = rp.0.add(off).sub(eye);
        let len = d.length();
        if len < 1e-6 {
            continue;
        }
        // look_direction 的逆：yaw 零点朝 -Z。
        let yaw_t = d.x.atan2(-d.z).to_degrees();
        let pitch_t = (d.y / len).clamp(-1.0, 1.0).asin().to_degrees();
        let dx = f64::from(wrap180(yaw_t - cur_yaw)) / deg_eff;
        let dy = -f64::from(pitch_t - cur_pitch) / deg_eff;
        if dx.is_finite() && dy.is_finite() {
            ctl.cands.push((u64::from(tgt.id), dx, dy));
        }
    }

    let Some((id, dx, dy)) = pick_target(&ctl.cands, ctl.hum.locked, fov_counts) else {
        if let Some(lk) = ctl.hum.locked.take() {
            ctl.hum.stop();
            ctl.con.reset();
            ctl.hist.clear();
            ctl.log
                .line(&format!("UNLOCK t={t:.3} id={lk} why=dead_or_out_of_fov"));
            ctl.log.flush();
        }
        return;
    };
    if ctl.hum.locked != Some(id) {
        if let Some(old) = ctl.hum.locked {
            ctl.log.line(&format!("UNLOCK t={t:.3} id={old} why=switch"));
        }
        ctl.hum.locked = Some(id);
        // 新目标：StopAiming +（Rain）反应延迟 /（匀速系）包络归零。
        ctl.hum.retarget(t, &tune);
        ctl.con.reset();
        ctl.hist.clear();
        ctl.log.line(&format!(
            "LOCK t={t:.3} id={id} dist={:.1}",
            dx.hypot(dy)
        ));
    }

    // 锁定目标：更新位置历史 + 取所选部位命中容差 w + 重算带提前量
    // 的驱动误差（hitscan 的扳机仍按无提前量误差判）。
    // intercept 提前量：Rain 用状态机剩余时间；匀速系 ≈ 剩余距离/当前速度。
    let lead = if algo == AimAlgo::Rain {
        ctl.hum.est_lead(&tune).min(0.3)
    } else {
        (dx.hypot(dy) / ctl.con.speed_now(&tune, algo).max(1.0)).clamp(0.0, 0.3)
    };
    let mut w = 1.0f64;
    let (mut ldx, mut ldy) = (dx, dy);
    for (tgt, rp, hb) in targets.iter() {
        if tgt.id == id as u32 {
            ctl.hist.push_back((t, rp.0));
            while ctl
                .hist
                .front()
                .is_some_and(|(ft, _)| t - ft > VEL_HIST_SECS)
            {
                ctl.hist.pop_front();
            }
            let (off, pr) = aim_part(hb, part);
            let d = rp.0.add(off).add(vel.scale(lead as f32)).sub(eye);
            let len = d.length();
            if len > 1e-6 {
                let yaw_t = d.x.atan2(-d.z).to_degrees();
                let pitch_t = (d.y / len).clamp(-1.0, 1.0).asin().to_degrees();
                ldx = f64::from(wrap180(yaw_t - cur_yaw)) / deg_eff;
                ldy = -f64::from(pitch_t - cur_pitch) / deg_eff;
            }
            w = hit_tolerance_counts(pr, rp.0.sub(eye).length(), deg_eff);
            break;
        }
    }

    // 运行中把算法切回 Rain：`hum` 在匀速系驱动期间停在 Idle，
    // 不补 retarget 会永久不动。
    if algo == AimAlgo::Rain && ctl.hum.phase_name() == "Idle" {
        ctl.hum.retarget(t, &tune);
    }
    let dt = time.delta_secs_f64();
    let (ix, iy) = if algo == AimAlgo::Rain {
        ctl.hum.step(ldx, ldy, dt, w, &tune, t)
    } else {
        ctl.con.step(ldx, ldy, dt, &tune, algo)
    };
    if ix != 0 || iy != 0 {
        motion.write(MouseMotion {
            delta: Vec2::new(ix as f32, iy as f32),
        });
        ctl.moved.0 += i64::from(ix);
        ctl.moved.1 += i64::from(iy);
    }
    // 逐帧行：阶段/误差/输出/朝向——排查抖动和天旋地转就靠它重建。
    ctl.log.frames += 1;
    let ph = if algo == AimAlgo::Rain {
        ctl.hum.phase_name()
    } else {
        ConstAim::phase_name(algo)
    };
    let d = ctl.hum.last_dbg;
    ctl.log.line(&format!(
        "F t={t:.3} ph={ph} id={id} dx={ldx:.1} dy={ldy:.1} w={w:.1} out={ix},{iy} yaw={cur_yaw:.2} pit={cur_pitch:.2} stp={:.1},{:.1} trm={:.1},{:.1} pnd={:.1},{:.1} dt={dt:.3}",
        d.0, d.1, d.2, d.3, d.4, d.5
    ));

    // 开火：倒计时内/未武装/按住中都不开；扳机条件 = 保持态残余 ≤ w。
    // 用无提前量的原始误差（hitscan 打的是目标当前位置），容差收 0.8
    // 留 tremor/帧延迟的余量，避免在命中圈边缘开火擦边 miss。
    let click = if algo == AimAlgo::Rain {
        ctl.hum.should_click(dx, dy, w * 0.8, &tune, t)
    } else {
        ctl.con.should_click(dx, dy, w * 0.8, &tune, t)
    };
    if clock.countdown <= 0.0 && gate.armed && ctl.holding_until.is_none() && click {
        let hold = if algo == AimAlgo::Rain {
            ctl.hum.click_hold_secs(t)
        } else {
            ctl.con.click_hold_secs(t)
        };
        mouse.press(MouseButton::Left);
        ctl.hold_pressed_at = t;
        ctl.holding_until = Some(t + hold);
        ctl.log.line(&format!(
            "FIRE t={t:.3} id={id} err={dx:.1},{dy:.1} w={w:.1} hold={:.0}ms",
            hold * 1000.0
        ));
    }
}

/// 自瞄 FOV 指示圈：单位圆环按当前投影（含 ADS 变焦）缩放成
/// 角半径 `settings.aimbot_fov_deg` 的屏幕椭圆。武装且开启显示
/// 时可见——与是否按住激活键无关，方便先看圈再按键。
pub(crate) fn aimbot_fov_ring(
    launch: Res<ViewportLaunch>,
    settings: Res<SettingsRes>,
    clock: Res<MatchClock>,
    cam: Query<&Projection, With<WorldCam>>,
    mut ring: Query<(&mut Transform, &mut Visibility), With<AimbotFovRing>>,
) {
    let Ok((mut tf, mut vis)) = ring.single_mut() else {
        return;
    };
    let show = launch.aimbot && settings.0.aimbot_fov_visible && !clock.finished;
    if !show {
        if *vis == Visibility::Visible {
            *vis = Visibility::Hidden;
        }
        return;
    }
    let Ok(proj) = cam.single() else {
        return;
    };
    let Projection::Perspective(p) = proj else {
        return;
    };
    let w = settings.0.render_width.max(1) as f32;
    let h = settings.0.render_height.max(1) as f32;
    let tan_v = (p.fov * 0.5).tan().max(1e-4);
    let tan_r = settings.0.aimbot_fov_deg.to_radians().tan();
    // 角半径 → 屏幕 px：y 走垂直半 FOV，x 走水平半 FOV（=tan(v/2)·aspect）。
    let ry = tan_r * (h * 0.5) / tan_v;
    let rx = tan_r * (w * 0.5) / (tan_v * p.aspect_ratio.max(1e-4));
    tf.scale = Vec3::new(rx, ry, 1.0);
    if *vis != Visibility::Visible {
        *vis = Visibility::Visible;
    }
}

/// 部位选择 → (命中体偏移, 该部位等效半径)。人形结构取对应命中体，
/// 非人形（纯球）各部位都回退 (0, 最大半径)。
fn aim_part(hb: &Hitboxes, part: AimbotPart) -> (crate::vec3::Vec3, f32) {
    match part {
        AimbotPart::Head => hb
            .0
            .iter()
            .filter_map(|h| match h {
                Hitbox::OffsetSphere { radius, offset } => Some((*offset, *radius)),
                _ => None,
            })
            .max_by(|a, b| a.0.y.partial_cmp(&b.0.y).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((crate::vec3::Vec3::ZERO, max_hit_radius(hb))),
        AimbotPart::Body => hb
            .0
            .iter()
            .find_map(|h| match h {
                Hitbox::Capsule {
                    radius, offset_y, ..
                } => Some((crate::vec3::Vec3::new(0.0, *offset_y, 0.0), *radius)),
                _ => None,
            })
            .unwrap_or((crate::vec3::Vec3::ZERO, max_hit_radius(hb))),
        AimbotPart::Center => (crate::vec3::Vec3::ZERO, max_hit_radius(hb)),
    }
}

/// 目标命中容差（counts）：目标角半径（度）/ deg_eff。
/// 注意 `atan` 返回弧度，必须 `to_degrees`——此前漏换导致 w 恒为
/// 1 count（~0.06°），扳机几乎永不满足、reflick 阈值仅 4 counts，
/// 表现为"按住就抖/瞄得比手动慢"的直接根因。
fn hit_tolerance_counts(radius: f32, dist: f32, deg_eff: f64) -> f64 {
    let ang = f64::from((radius / dist.max(1e-3)).atan());
    (ang.to_degrees() / deg_eff).max(1.0)
}

/// 速度模长钳制：瞬移类运动模型的差分速度是物理上不可能的，
/// 钳掉以免提前量把瞄准点甩飞。
fn cap_vel(v: crate::vec3::Vec3, max: f32) -> crate::vec3::Vec3 {
    let len = v.length();
    if len > max && len > 1e-6 {
        v.scale(max / len)
    } else {
        v
    }
}

/// 位置历史差分估计目标速度（窗内首末差/时间跨度）。
fn estimate_vel(hist: &VecDeque<(f64, crate::vec3::Vec3)>) -> crate::vec3::Vec3 {
    let (Some((t0, p0)), Some((t1, p1))) = (hist.front(), hist.back()) else {
        return crate::vec3::Vec3::ZERO;
    };
    let span = t1 - t0;
    if span < 0.02 {
        return crate::vec3::Vec3::ZERO;
    }
    p1.sub(*p0).scale((1.0 / span) as f32)
}

/// 命中容差用的目标半径：取所有命中区域的最大半径。
fn max_hit_radius(hb: &Hitboxes) -> f32 {
    hb.0.iter()
        .map(|h| match h {
            Hitbox::Sphere { radius }
            | Hitbox::OffsetSphere { radius, .. }
            | Hitbox::Capsule { radius, .. } => *radius,
        })
        .fold(0.0f32, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 命中容差换算：角半径（度）/ deg_eff，且保证 ≥1 count 下限。
    /// 回归：曾把 asin 的弧度直接除 deg/count，w 恒为 1 → 永不达容差。
    #[test]
    fn hit_tolerance_counts_should_convert_degrees() {
        // r=0.3m @ 10m → atan(0.03)≈1.72°；deg_eff=0.06 → ~28.7 counts。
        let w = hit_tolerance_counts(0.3, 10.0, 0.06);
        assert!((w - 28.65).abs() < 0.5, "w={w}");
        // 超小目标/超远 → 下限 1。
        assert_eq!(hit_tolerance_counts(0.001, 100.0, 0.06), 1.0);
    }

    /// 速度钳制：超速向量按模长缩回上限，零/低速不动。
    #[test]
    fn cap_vel_should_clamp_magnitude() {
        let v = crate::vec3::Vec3::new(30.0, 40.0, 0.0); // |v|=50
        let c = cap_vel(v, 25.0);
        assert!((c.length() - 25.0).abs() < 1e-4);
        let slow = crate::vec3::Vec3::new(3.0, 4.0, 0.0);
        assert_eq!(cap_vel(slow, 25.0).length(), slow.length());
        assert_eq!(cap_vel(crate::vec3::Vec3::ZERO, 25.0).length(), 0.0);
    }
}
