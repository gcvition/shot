//! 仓库根 / 资源 / 缓存路径。
//!
//! [`AppPaths::discover`] 的查找顺序：
//! 1. exe 旁边有 `res/`（用户解压后的用法）
//! 2. `CARGO_MANIFEST_DIR/res`（你在 IDE 里 `cargo run`）
//! 3. 当前工作目录
//!
//! 设置、成绩、回放都进 `cache/`，不要往 `res/` 里写运行时文件。

use std::path::{Path, PathBuf};

use crate::error::Result;

/// 一次运行里所有「文件在哪」的集合。克隆很便宜（都是 PathBuf）。
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub root: PathBuf,
    pub res: PathBuf,
    pub cache: PathBuf,
    pub sessions: PathBuf,
    pub settings: PathBuf,
    pub stats_db: PathBuf,
    pub last_session: PathBuf,
}

impl AppPaths {
    /// 找到带 `res/` 的根目录。失败几乎只会发生在工作目录完全不对的时候。
    pub fn discover() -> Result<Self> {
        let exe_dir = std::env::current_exe()?
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = if exe_dir.join("res").is_dir() {
            exe_dir
        } else if manifest.join("res").is_dir() {
            manifest
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        };
        Ok(Self::from_root(root))
    }

    pub fn from_root(root: PathBuf) -> Self {
        let cache = root.join("cache");
        Self {
            res: root.join("res"),
            settings: cache.join("settings.json"),
            stats_db: cache.join("stats.db"),
            sessions: cache.join("sessions"),
            last_session: cache.join("last_session.json"),
            cache,
            root,
        }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.cache)?;
        std::fs::create_dir_all(&self.sessions)?;
        let _ = crate::assets::ensure_asset_dirs(self);
        Ok(())
    }

    /// 把 `res/themes/主题.json` 或 `themes/主题.json` 收成绝对路径。
    pub fn resolve_res(&self, relative: &str) -> PathBuf {
        let as_path = Path::new(relative);
        if as_path.is_absolute() && as_path.exists() {
            return as_path.to_path_buf();
        }
        let stripped = relative
            .strip_prefix("res/")
            .or_else(|| relative.strip_prefix("res\\"))
            .unwrap_or(relative);
        self.res.join(stripped)
    }

    pub fn session_trace(&self, id: &str) -> PathBuf {
        self.sessions.join(format!("{id}.shot"))
    }
}

#[cfg(test)]
mod tests {
    use super::AppPaths;

    #[test]
    fn from_root_should_place_sessions_under_cache() {
        let paths = AppPaths::from_root(std::path::PathBuf::from("C:/game/shot"));
        assert_eq!(
            paths.session_trace("abc"),
            std::path::PathBuf::from("C:/game/shot/cache/sessions/abc.shot")
        );
    }

    #[test]
    fn resolve_res_should_join_nested_theme_path() {
        let paths = AppPaths::from_root(std::path::PathBuf::from("C:/game/shot"));
        assert_eq!(
            paths.resolve_res("res/themes/主题.json"),
            std::path::PathBuf::from("C:/game/shot/res/themes/主题.json")
        );
    }
}
