//! 玩家身体和视角输入。
//!
//! [`PlayerBody`] 是玩家的物理状态：站桩场景（Sixshot/Gridshot）里 `pos`
//! 永远是地面原点、`vel` 是零，行为和旧的 `PLAYER_EYE` 常量完全一致；
//! 可移动场景（M4 起）每 tick 更新 `prev_pos`/`pos`/`vel`。
//!
//! [`Look`] 是鼠标直接控制的朝向，[`ViewPunch`] 是武器后坐力顶出来的
//! 额外偏移（M2）。两者分开存：回放和分析需要分清「手动输入」和「枪顶的」。

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::scenario::look_direction;
use crate::session::MouseSample;
use crate::vec3::Vec3 as V3;

use super::{LiveTrace, MatchClock};

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
}

impl Default for PlayerBody {
    fn default() -> Self {
        Self {
            pos: V3::ZERO,
            vel: V3::ZERO,
            prev_pos: V3::ZERO,
            on_ground: true,
            crouch_frac: 0.0,
        }
    }
}

impl PlayerBody {
    /// 当前眼睛位置。站桩场景恒等于旧的 `PLAYER_EYE`。
    pub(crate) fn eye(&self) -> V3 {
        V3::new(self.pos.x, self.pos.y + EYE_HEIGHT, self.pos.z)
    }

    /// 固定步长模拟下的渲染插值眼睛位置。M0 没有移动，`alpha` 被忽略；
    /// M4 起返回 `prev_pos.lerp(pos, alpha) + 眼高`。
    #[allow(dead_code)]
    pub(crate) fn eye_interp(&self, _alpha: f32) -> V3 {
        self.eye()
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

/// 武器后坐力造成的视角偏移（度）。M2 起由武器系统写；
/// 有效视线 = `look_direction(yaw + punch.yaw, pitch + punch.pitch)`。
#[derive(Resource, Default)]
#[allow(dead_code)]
pub(crate) struct ViewPunch {
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
    mut q: Query<&mut Transform, With<WorldCam>>,
) {
    let Ok(mut tf) = q.single_mut() else {
        return;
    };
    let dir = look_direction(look.yaw, look.pitch);
    let eye = body.eye();
    let target = eye.add(V3::new(dir.x, dir.y, dir.z));
    tf.look_at(Vec3::new(target.x, target.y, target.z), Vec3::Y);
}
