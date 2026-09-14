use std::path::PathBuf;
use std::process::{Command, ExitStatus};

use crate::error::{Result, ShotError};
use crate::paths::AppPaths;
use crate::settings::ScenarioKind;

pub fn current_exe() -> Result<PathBuf> {
    Ok(std::env::current_exe()?)
}

pub fn run_play(scenario: ScenarioKind, session_id: &str) -> Result<ExitStatus> {
    let exe = current_exe()?;
    let status = Command::new(exe)
        .args(["play", scenario.as_str(), session_id])
        .status()?;
    Ok(status)
}

pub fn run_replay(session_id: &str) -> Result<ExitStatus> {
    let exe = current_exe()?;
    let status = Command::new(exe).args(["replay", session_id]).status()?;
    Ok(status)
}

pub fn spawn_encode(session_id: &str) -> Result<()> {
    let exe = current_exe()?;
    Command::new(exe).args(["encode", session_id]).spawn()?;
    Ok(())
}

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
    Ok(value
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::to_string))
}

pub fn open_in_explorer(path: &PathBuf) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(ShotError::from)?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err(ShotError::Other("open folder is Windows-only in v1".into()))
    }
}
