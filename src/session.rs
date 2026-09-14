use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::fov::FovKind;
use crate::paths::AppPaths;
use crate::settings::{DisplayMode, ScenarioKind, Settings};
use crate::vec3::Vec3;

pub const SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub cm_per_360: f64,
    pub dpi: u32,
    pub fov_deg: f32,
    pub fov_kind: FovKind,
    pub render_width: u32,
    pub render_height: u32,
    pub display_mode: DisplayMode,
    pub window_width: u32,
    pub window_height: u32,
    pub refresh_hz: u32,
}

impl SettingsSnapshot {
    pub fn from_settings(settings: &Settings, window_w: u32, window_h: u32, refresh_hz: u32) -> Self {
        Self {
            cm_per_360: settings.cm_per_360,
            dpi: settings.dpi,
            fov_deg: settings.fov_deg,
            fov_kind: settings.fov_kind,
            render_width: settings.render_width,
            render_height: settings.render_height,
            display_mode: settings.display_mode,
            window_width: window_w,
            window_height: window_h,
            refresh_hz,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionHeader {
    pub schema_version: u16,
    pub session_id: String,
    pub scenario: ScenarioKind,
    pub started_at_unix_ms: i64,
    pub duration_ms: u32,
    pub rng_seed: u64,
    pub settings: SettingsSnapshot,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct MouseSample {
    pub t_us: u64,
    pub dx: f32,
    pub dy: f32,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ShotEvent {
    pub t_us: u64,
    pub hit: bool,
    pub target_id: u32,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum WorldKind {
    Spawn,
    Despawn,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WorldEvent {
    pub t_us: u64,
    pub kind: WorldKind,
    pub id: u32,
    pub pos: Vec3,
    pub radius: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionFile {
    pub header: SessionHeader,
    pub mouse: Vec<MouseSample>,
    pub shots: Vec<ShotEvent>,
    pub world: Vec<WorldEvent>,
}

impl SessionFile {
    pub fn new(
        session_id: String,
        scenario: ScenarioKind,
        rng_seed: u64,
        settings: SettingsSnapshot,
    ) -> Self {
        Self {
            header: SessionHeader {
                schema_version: SCHEMA_VERSION,
                session_id,
                scenario,
                started_at_unix_ms: chrono::Utc::now().timestamp_millis(),
                duration_ms: 0,
                rng_seed,
                settings,
            },
            mouse: Vec::with_capacity(64_000),
            shots: Vec::with_capacity(512),
            world: Vec::with_capacity(512),
        }
    }

    pub fn save(&self, paths: &AppPaths) -> Result<()> {
        paths.ensure_dirs()?;
        let bytes = postcard::to_allocvec(self)?;
        std::fs::write(paths.session_trace(&self.header.session_id), bytes)?;
        Ok(())
    }

    pub fn load(paths: &AppPaths, session_id: &str) -> Result<Self> {
        let path = paths.session_trace(session_id);
        let bytes = std::fs::read(&path)?;
        Ok(postcard::from_bytes(&bytes)?)
    }

    pub fn look_at(&self, t_us: u64) -> (f32, f32) {
        if self.mouse.is_empty() {
            return (0.0, 0.0);
        }
        match self.mouse.binary_search_by(|s| s.t_us.cmp(&t_us)) {
            Ok(i) => (self.mouse[i].yaw_deg, self.mouse[i].pitch_deg),
            Err(0) => (self.mouse[0].yaw_deg, self.mouse[0].pitch_deg),
            Err(i) if i >= self.mouse.len() => {
                let last = self.mouse.last().expect("mouse not empty");
                (last.yaw_deg, last.pitch_deg)
            }
            Err(i) => {
                let a = &self.mouse[i - 1];
                let b = &self.mouse[i];
                let span = (b.t_us - a.t_us).max(1) as f32;
                let t = (t_us.saturating_sub(a.t_us)) as f32 / span;
                (
                    a.yaw_deg + (b.yaw_deg - a.yaw_deg) * t,
                    a.pitch_deg + (b.pitch_deg - a.pitch_deg) * t,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionFile, SettingsSnapshot};
    use crate::fov::FovKind;
    use crate::paths::AppPaths;
    use crate::settings::{DisplayMode, ScenarioKind, Settings};

    #[test]
    fn session_roundtrip_should_preserve_mouse_samples() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let snapshot = SettingsSnapshot::from_settings(&Settings::default(), 1920, 1080, 144);
        let mut file = SessionFile::new("s1".into(), ScenarioKind::SixTargets, 42, snapshot);
        file.mouse.push(super::MouseSample {
            t_us: 0,
            dx: 1.0,
            dy: 0.0,
            yaw_deg: 0.5,
            pitch_deg: 0.0,
        });
        file.save(&paths).unwrap();
        let loaded = SessionFile::load(&paths, "s1").unwrap();
        assert_eq!(loaded.header.rng_seed, 42);
        assert_eq!(loaded.mouse.len(), 1);
        assert!((loaded.mouse[0].yaw_deg - 0.5).abs() < 1e-6);
        assert_eq!(loaded.header.settings.fov_kind, FovKind::HorizontalRes);
        assert_eq!(loaded.header.settings.display_mode, DisplayMode::BorderlessStretch);
    }
}
