//! 玩家身体和视角输入。
//!
//! [`PlayerBody`] 是玩家的物理状态：站桩场景（Sixshot/Gridshot）里 `pos`
//! 永远是地面原点、`vel` 是零，行为和旧的 `PLAYER_EYE` 常量完全一致；
//! 可移动场景（M4 起）每 tick 更新 `prev_pos`/`pos`/`vel`。
//!
//! `Look` 是鼠标直接控制的朝向；武器后坐的显示偏移在 `WeaponState` 里（M2，
//! `punch_yaw`/`punch_pitch`）。两者分开存：回放和分析需要分清「手动输入」和「枪顶的」。

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::scenario::look_direction;
use crate::session::MouseSample;
use crate::vec3::Vec3 as V3;

use super::{LiveTrace, MatchClock, SettingsRes, ViewportLaunch};

/// 站立时眼睛离脚底的高度（米）。等于旧的 `PLAYER_EYE.y`。
pub(crate) const EYE_HEIGHT: f32 = crate::scenario::PLAYER_EYE.y;

/// 玩家的物理状态。`pos` 是 **脚底** 位置；眼睛 = `pos + EYE_HEIGHT`。
/// `vel`/`prev_pos`/`on_ground`/`crouch_frac` 由 M4 移动系统读写。
#[derive(Resource, Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct PlayerBody {
    pub pos: V3,
    pub vel: V3,
    pub prev_pos: V3,
    pub on_ground: bool,
    /// 0 = 站直，1 = 全蹲（蹲起过渡的中间值）。
    pub crouch_frac: f32,
    /// 滑铲截止时刻（sim 秒）；0 = 未滑铲（M8）。
    pub slide_until: f32,
    /// 上一次记脚步声的累计位移（M8）。
    pub step_acc: f32,
    /// 上 tick 的按键掩码（WASDSPCtrlCShift 位图）：固定步长下去抖（M8）。
    pub keys_mask: u8,
}

impl Default for PlayerBody {
    fn default() -> Self {
        Self {
            pos: V3::ZERO,
            vel: V3::ZERO,
            prev_pos: V3::ZERO,
            on_ground: true,
            slide_until: 0.0,
            step_acc: 0.0,
            keys_mask: 0,
            crouch_frac: 0.0,
        }
    }
}

impl PlayerBody {
    /// 当前眼睛位置。站桩场景恒等于旧的 `PLAYER_EYE`；蹲下眼高降 0.6m。
    pub(crate) fn eye(&self) -> V3 {
        V3::new(
            self.pos.x,
            self.pos.y + EYE_HEIGHT - 0.6 * self.crouch_frac,
            self.pos.z,
        )
    }

    /// 固定步长模拟下的渲染插值眼睛位置（M4）。
    pub(crate) fn eye_interp(&self, alpha: f32) -> V3 {
        let p = self.prev_pos.add(self.pos.sub(self.prev_pos).scale(alpha));
        V3::new(p.x, p.y + EYE_HEIGHT - 0.6 * self.crouch_frac, p.z)
    }
}

/// 鼠标移动 1 个 count 时，视角转多少度。来自厘米/360 和 DPI。
#[derive(Resource)]
pub(crate) struct DegPerCount(pub f64);

/// 当前朝向。yaw 左右、pitch 上下，单位都是度。只反映鼠标输入。
#[derive(Resource)]
pub(crate) struct Look {
    pub yaw: f32,
    pub pitch: f32,
}

/// 防止「从菜单点进来时左键还按着」立刻开一枪。
///
/// 必须窗口已聚焦 **并且** 当前没有按着左键，才把 `armed` 设为 true。
#[derive(Resource, Default)]
pub(crate) struct FireGate {
    pub armed: bool,
}

/// 从菜单点进来时左键可能还按着：必须聚焦且左键已松开，才允许后续开火。
pub(crate) fn should_arm_fire_gate(focused: bool, left_held: bool) -> bool {
    focused && !left_held
}

pub(crate) fn arm_fire_gate(
    mut gate: ResMut<FireGate>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if gate.armed {
        return;
    }
    let focused = windows.iter().any(|window| window.focused);
    if should_arm_fire_gate(focused, mouse.pressed(MouseButton::Left)) {
        gate.armed = true;
    }
}

/// 每帧把鼠标位移累加到 yaw/pitch。倒计时期间也转视角，但不写入轨迹。
pub(crate) fn apply_mouse_look(
    mut motion: MessageReader<MouseMotion>,
    mut look: ResMut<Look>,
    deg: Res<DegPerCount>,
    time: Res<Time>,
    mut trace: ResMut<LiveTrace>,
    clock: Res<MatchClock>,
    gate: Res<FireGate>,
    mut wstate: ResMut<super::combat::WeaponState>,
    launch: Res<ViewportLaunch>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    if clock.finished {
        for _ in motion.read() {}
        return;
    }
    if clock.countdown > 0.0 {
        for event in motion.read() {
            let (yaw, pitch) = crate::sensitivity::apply_look(
                look.yaw,
                look.pitch,
                event.delta.x,
                event.delta.y,
                deg.0,
            );
            look.yaw = yaw;
            look.pitch = pitch;
        }
        return;
    }
    if !gate.armed {
        for _ in motion.read() {}
        return;
    }
    let t_us = (time.elapsed_secs() * 1_000_000.0) as u64;
    let negatable = launch.scenario.weapon.recoil.negatable;
    // ADS：右键按住且武器有 ads 配置 → 灵敏度乘 sens_mult（M8）。
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
    for event in motion.read() {
        let (yaw, pitch) = crate::sensitivity::apply_look(
            look.yaw,
            look.pitch,
            event.delta.x,
            event.delta.y,
            deg_eff,
        );
        // RecoilNegatable：鼠标反向移动直接吃掉等量的累积后坐（Apex/OW 语义）。
        if negatable && (wstate.acc_pitch != 0.0 || wstate.acc_yaw != 0.0) {
            let dp = pitch - look.pitch;
            let dy = yaw - look.yaw;
            // punch = acc*view_scale，鼠标抵消量要先换回 acc 单位。
            let vs = launch.scenario.weapon.recoil.view_scale.max(0.01);
            wstate.acc_pitch = (wstate.acc_pitch + dp / vs).max(0.0);
            wstate.acc_yaw += dy / vs;
            wstate.punch_pitch = wstate.acc_pitch * vs;
            wstate.punch_yaw = wstate.acc_yaw * vs;
        }
        look.yaw = yaw;
        look.pitch = pitch;
        trace.file.mouse.push(MouseSample {
            t_us,
            dx: event.delta.x,
            dy: event.delta.y,
            yaw_deg: yaw,
            pitch_deg: pitch,
        });
    }
}

/// 画 3D 世界的相机。每帧 `look_at` 准星方向。
#[derive(Component)]
pub(crate) struct WorldCam;

/// 用 `Transform::look_at` 对准视线方向。不要改 Camera 的投影矩阵来「转头」。
pub(crate) fn update_camera(
    look: Res<Look>,
    body: Res<PlayerBody>,
    wstate: Res<super::combat::WeaponState>,
    fixed: Res<Time<Fixed>>,
    replay: Option<Res<super::replay::ReplayCtl>>,
    mut q: Query<&mut Transform, With<WorldCam>>,
) {
    let Ok(mut tf) = q.single_mut() else {
        return;
    };
    // 视角冲量（后坐力的显示部分）叠在鼠标朝向上。
    let dir = look_direction(look.yaw + wstate.punch_yaw, look.pitch + wstate.punch_pitch);
    let eye = body.eye_interp(fixed.overstep_fraction());
    let mut cam_pos = eye;
    // M8：回放按 V 切第三人称（镜头拉到玩家身后上方）。
    if replay.is_some_and(|r| r.third_person) {
        cam_pos = V3::new(
            eye.x - dir.x * 3.5,
            eye.y - dir.y * 3.5 + 1.2,
            eye.z - dir.z * 3.5,
        );
    }
    tf.translation = Vec3::new(cam_pos.x, cam_pos.y, cam_pos.z);
    let target = eye.add(V3::new(dir.x, dir.y, dir.z));
    tf.look_at(Vec3::new(target.x, target.y, target.z), Vec3::Y);
}

/// ADS 变焦（M8）：右键按住且武器配了  → 垂直 FOV 换成 zoom_fov。
pub(crate) fn ads_zoom(
    mouse: Res<ButtonInput<MouseButton>>,
    launch: Res<ViewportLaunch>,
    settings: Res<SettingsRes>,
    mut q: Query<&mut Projection, With<WorldCam>>,
) {
    let Ok(mut proj) = q.single_mut() else {
        return;
    };
    let Projection::Perspective(p) = proj.as_mut() else {
        return;
    };
    let s = &settings.0;
    let aspect = s.render_width.max(1) as f32 / s.render_height.max(1) as f32;
    let base = crate::fov::vertical_fov_radians(s.fov_deg, s.fov_kind, aspect);
    p.fov = if mouse.pressed(MouseButton::Right) {
        launch
            .scenario
            .weapon
            .ads
            .map(|a| a.zoom_fov.to_radians())
            .unwrap_or(base)
    } else {
        base
    };
}

/// 玩家移动（M4，FixedUpdate 240Hz）：读 WASD/Space/Ctrl，
/// 按预设摩擦/加速模型推进 PlayerBody，再和地图 AABB 碰撞解算。
/// Static 预设直接返回（站桩场景零开销）。每 tick 写一条 PlayerSample。
#[allow(clippy::too_many_arguments)]
pub(crate) fn move_player(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut body: ResMut<PlayerBody>,
    look: Res<Look>,
    mut trace: ResMut<LiveTrace>,
    director: Res<super::SpawnDirector>,
    launch: Res<ViewportLaunch>,
    clock: Res<MatchClock>,
    audio: Res<super::combat::AudioBank>,
    php: Res<super::combat::PlayerHp>,
) {
    use crate::sce::MovementPreset;
    let spec = &launch.scenario;
    if clock.finished || clock.countdown > 0.0 {
        return;
    }
    if spec.player.movement_preset == MovementPreset::Static {
        return;
    }
    const DT: f32 = 1.0 / 240.0;
    let t = director.sim_time;
    // WASD → wish 方向（look yaw 旋转）。
    let fwd = (keyboard.pressed(KeyCode::KeyW) as i8 - keyboard.pressed(KeyCode::KeyS) as i8) as f32;
    let right = (keyboard.pressed(KeyCode::KeyD) as i8 - keyboard.pressed(KeyCode::KeyA) as i8) as f32;
    let wish = crate::sim::movement::wish_dir(fwd, right, look.yaw);
    let jump = keyboard.pressed(KeyCode::Space);
    let mut crouch = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::KeyC);
    // 滑铲（M8）：Shift + 在地面 + 有速度 → 短促冲一段，期间按蹲处理。
    let speed2d = (body.vel.x * body.vel.x + body.vel.z * body.vel.z).sqrt();
    // 沿检测用自记掩码：FixedUpdate 里 just_pressed 会跨多个 tick 重复（M8 修）。
    let shift_edge = keyboard.pressed(KeyCode::ShiftLeft) && body.keys_mask & 0x80 == 0;
    if shift_edge && body.on_ground && speed2d > 3.0 {
        body.slide_until = t + 0.55;
        let dir = V3::new(body.vel.x, 0.0, body.vel.z).normalized();
        body.vel.x = dir.x * (speed2d * 1.35).min(9.0);
        body.vel.z = dir.z * (speed2d * 1.35).min(9.0);
    }
    if t < body.slide_until {
        crouch = true;
    }
    let p = crate::sim::movement::params(spec.player.movement_preset);

    let (mut pos, vel, ground) = crate::sim::movement::step(
        body.pos,
        body.vel,
        wish,
        jump && body.on_ground,
        crouch,
        body.on_ground,
        DT,
        &p,
    );
    let (pos2, landed) = crate::sim::movement::resolve_blocks(
        pos,
        crate::sim::movement::PLAYER_HALF,
        crate::sim::movement::PLAYER_HEIGHT,
        &spec.map.blocks,
        0.4,
    );
    pos = pos2;
    let (vel, ground) = if landed && vel.y <= 0.0 {
        (V3::new(vel.x, 0.0, vel.z), true)
    } else {
        (vel, ground)
    };
    pos = crate::sim::movement::clamp_bounds(pos, crate::sim::movement::PLAYER_HALF, spec.map.bounds);
    // 脚步声（M8）：地面移动每累计 1.6m 触发一次。
    {
        let moved = (pos.x - body.pos.x).hypot(pos.z - body.pos.z);
        body.step_acc += moved;
        if body.on_ground && body.step_acc > 1.6 {
            body.step_acc = 0.0;
            let _ = audio.tx.send(crate::sfx::SfxEvent::Footstep { pan: 0.0, vol: 0.4 });
        }
    }

    body.prev_pos = body.pos;
    body.pos = pos;
    body.vel = vel;
    body.on_ground = ground;
    let target_crouch = if crouch { 1.0 } else { 0.0 };
    body.crouch_frac += (target_crouch - body.crouch_frac) * (DT * 10.0).min(1.0);

    // 每 tick 写一条 PlayerSample；按键变化写 KeyEvent。
    let t_us = (t * 1_000_000.0) as u64;
    let flags = if ground {
        crate::session::player_flags::ON_GROUND
    } else {
        0
    } | if crouch {
        crate::session::player_flags::CROUCH
    } else {
        0
    };
    trace.file.player.push(crate::session::PlayerSample {
        t_us,
        pos: body.pos,
        vel: body.vel,
        flags,
        hp: php.hp,
    });
    // 按键边沿用自记掩码：240Hz 下 just_pressed/just_released 会重复写（M8 修）。
    const KEY_TAB: [(u8, KeyCode, &str); 8] = [
        (0x01, KeyCode::KeyW, "forward"),
        (0x02, KeyCode::KeyS, "back"),
        (0x04, KeyCode::KeyA, "left"),
        (0x08, KeyCode::KeyD, "right"),
        (0x10, KeyCode::Space, "jump"),
        (0x20, KeyCode::ControlLeft, "crouch"),
        (0x40, KeyCode::KeyC, "crouch"),
        (0x80, KeyCode::ShiftLeft, "slide"),
    ];
    for (bit, key, action) in KEY_TAB {
        let down = keyboard.pressed(key);
        let was = body.keys_mask & bit != 0;
        if down != was {
            trace.file.keys.push(crate::session::KeyEvent {
                t_us,
                action: action.to_string(),
                down,
            });
            if down {
                body.keys_mask |= bit;
            } else {
                body.keys_mask &= !bit;
            }
        }
    }
}
