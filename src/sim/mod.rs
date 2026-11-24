//! 纯函数模拟层：不给 Bevy 依赖，确定可复现。
//!
//! - [`motion`]：目标运动模型（M1）。
//! - 后续：movement（玩家移动 M4）、collision、los、recoil、projectile（M2+）。

pub mod motion;
