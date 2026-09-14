use std::path::{Path, PathBuf};

use crate::error::Result;

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub root: PathBuf,
    pub res: PathBuf,
    pub cache: PathBuf,
    pub sessions: PathBuf,
    pub video: PathBuf,
    pub settings: PathBuf,
    pub stats_db: PathBuf,
    pub last_session: PathBuf,
}

impl AppPaths {
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
            video: cache.join("video"),
            last_session: cache.join("last_session.json"),
            cache,
            root,
        }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.cache)?;
        std::fs::create_dir_all(&self.sessions)?;
        std::fs::create_dir_all(&self.video)?;
        Ok(())
    }

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

    pub fn session_video(&self, id: &str) -> PathBuf {
        self.video.join(format!("{id}.mp4"))
    }
}

#[cfg(test)]
mod tests {
    use super::AppPaths;

    #[test]
    fn from_root_should_place_video_under_cache() {
        let paths = AppPaths::from_root(std::path::PathBuf::from("C:/game/shot"));
        assert_eq!(
            paths.session_video("abc"),
            std::path::PathBuf::from("C:/game/shot/cache/video/abc.mp4")
        );
    }
}
