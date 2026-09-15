//! 菜单进程再拉起 **同一份 exe**，用来开 Bevy 窗口。
//!
//! GPUI 和 Bevy 不能舒服地共处一个消息循环，所以训练不是「在菜单里嵌一块 3D」，
//! 而是 `Command::new(current_exe).args(["play", scenario, id])`。
//! `status()` 会阻塞，直到训练窗口关掉——这就是菜单会卡住的原因。
//!
//! `last_session.json` 记下刚刚打完的 id，菜单启动时用来跳到「记录」页。

use std::path::PathBuf;
use std::process::{Command, ExitStatus};

use crate::error::Result;
use crate::paths::AppPaths;

/// 当前正在运行的这份 `shot.exe`。开发时是 `target/debug/shot.exe`。
pub fn current_exe() -> Result<PathBuf> {
    Ok(std::env::current_exe()?)
}

/// 阻塞直到训练进程退出。成功不代表打完了，只代表窗口关掉了。
pub fn run_play(scenario: &str, session_id: &str) -> Result<ExitStatus> {
    let exe = current_exe()?;
    let status = Command::new(exe)
        .args(["play", scenario, session_id])
        .status()?;
    Ok(status)
}

/// 阻塞直到回放窗口退出。
pub fn run_replay(session_id: &str) -> Result<ExitStatus> {
    let exe = current_exe()?;
    let status = Command::new(exe).args(["replay", session_id]).status()?;
    Ok(status)
}

/// 训练结束时写下 id，菜单下次打开会读它。
pub fn write_last_session(paths: &AppPaths, id: &str) -> Result<()> {
    paths.ensure_dirs()?;
    std::fs::write(&paths.last_session, format!("{{\"id\":\"{id}\"}}"))?;
    Ok(())
}

pub fn read_last_session(paths: &AppPaths) -> Result<Option<String>> {
    if !paths.last_session.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&paths.last_session)?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    Ok(value.get("id").and_then(|v| v.as_str()).map(str::to_string))
}

pub fn clear_last_session(paths: &AppPaths) -> Result<()> {
    if paths.last_session.exists() {
        std::fs::remove_file(&paths.last_session)?;
    }
    Ok(())
}
