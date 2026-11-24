//! 纯函数模拟层：不给 Bevy 依赖，确定可复现。
//!
//! - `motion`：目标运动模型（M1）。
//! - `movement`：玩家移动 + AABB 碰撞（M4）。

pub mod motion;
pub mod movement;
