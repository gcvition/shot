//! 自瞄模块：按 Rain `docs/design/aimbot.md` 目标架构实现 + Bevy 接线。
//!
//! - [`humanizer`]：双子运动状态机（反应延迟 → Fitts 定时 →
//!   minimum-jerk 主运动 → 停顿 → P 修正 → OU tremor 保持态），
//!   与 Rain v0 `aim.rs` 接口同形、内部全换，见该文件头注释。
//! - [`system`]：[`aimbot_drive`] system，注入点 = 输入消息层
//!   （`MouseMotion` + `ButtonInput`），不绕过任何玩法系统。
//! - [`algo`]：匀速定步长系算法集（AFG/NEXO/Nep 三家参考实现的
//!   对照复刻），设置页 `aimbot_algo` 可切换，默认 Rain。
//!
//! 激活：设置开关/`--aimbot`/`SHOT_AIMBOT=1` 武装；局内按住
//! `settings.aimbot_key`（默认鼠标右键）生效，松开即 StopAiming。

mod algo;
mod humanizer;
mod log;
mod system;

pub use humanizer::{AimTune, Humanizer, RAIN_PARAMS, RainParam, pick_target};
pub(crate) use system::{AimbotCtl, aimbot_drive, aimbot_fov_ring};
