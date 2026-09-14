use crate::error::{Result, ShotError};
use crate::fov::FovKind;
use crate::paths::AppPaths;
use crate::sensitivity::{DEFAULT_CM_PER_360, DEFAULT_DPI};
use serde::{Deserialize, Serialize};

pub const SESSION_SECS: f32 = 60.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayMode {
    #[default]
    BorderlessStretch,
    Windowed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScenarioKind {
    #[default]
    SixTargets,
    GridShot,
}

impl ScenarioKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SixTargets => "six-targets",
            Self::GridShot => "grid-shot",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "six-targets" | "six" | "1w6ts" => Some(Self::SixTargets),
            "grid-shot" | "grid" | "gridshot" => Some(Self::GridShot),
            _ => None,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::SixTargets => "六目标",
            Self::GridShot => "网格射击",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub cm_per_360: f64,
    pub dpi: u32,
    pub fov_deg: f32,
    pub fov_kind: FovKind,
    pub render_width: u32,
    pub render_height: u32,
    pub display_mode: DisplayMode,
    pub theme_path: String,
    pub crosshair_path: String,
    pub hit_sound: String,
    pub miss_sound: String,
    pub master_volume: f32,
    pub video_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            cm_per_360: DEFAULT_CM_PER_360,
            dpi: DEFAULT_DPI,
            fov_deg: 103.0,
            fov_kind: FovKind::HorizontalRes,
            render_width: 1920,
            render_height: 1080,
            display_mode: DisplayMode::BorderlessStretch,
            theme_path: "res/主题.json".into(),
            crosshair_path: "res/准星.png".into(),
            hit_sound: "res/命中.wav".into(),
            miss_sound: "res/未命中.wav".into(),
            master_volume: 1.0,
            video_enabled: true,
        }
    }
}

impl Settings {
    pub fn load_or_default(paths: &AppPaths) -> Result<Self> {
        if paths.settings.exists() {
            let raw = std::fs::read_to_string(&paths.settings)?;
            let settings: Self = serde_json::from_str(&raw)?;
            settings.validate()?;
            Ok(settings)
        } else {
            let settings = Self::default();
            settings.save(paths)?;
            Ok(settings)
        }
    }

    pub fn save(&self, paths: &AppPaths) -> Result<()> {
        self.validate()?;
        paths.ensure_dirs()?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&paths.settings, json)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        crate::sensitivity::deg_per_count(self.cm_per_360, self.dpi)?;
        validate_resolution(self.render_width, self.render_height)?;
        if !(self.fov_deg.is_finite() && self.fov_deg > 10.0 && self.fov_deg < 170.0) {
            return Err(ShotError::Other(format!(
                "fov must be in (10, 170), got {}",
                self.fov_deg
            )));
        }
        if !(0.0..=1.0).contains(&self.master_volume) {
            return Err(ShotError::Other("volume must be 0..=1".into()));
        }
        Ok(())
    }

    pub fn render_aspect(&self) -> f32 {
        self.render_width as f32 / self.render_height as f32
    }

    pub const RESOLUTION_PRESETS: &[(u32, u32, &'static str)] = &[
        (1920, 1080, "1920×1080"),
        (1280, 960, "1280×960"),
        (1280, 720, "1280×720"),
        (1024, 768, "1024×768"),
        (1440, 1080, "1440×1080"),
        (1600, 900, "1600×900"),
    ];
}

pub fn validate_resolution(width: u32, height: u32) -> Result<()> {
    if width < 640 || height < 480 || width > 7680 || height > 4320 {
        return Err(ShotError::Resolution(width, height));
    }
    if width % 2 == 1 || height % 2 == 1 {
        return Err(ShotError::Resolution(width, height));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_resolution, Settings};

    #[test]
    fn default_settings_should_be_valid() {
        Settings::default().validate().unwrap();
    }

    #[test]
    fn odd_resolution_should_be_rejected() {
        assert!(validate_resolution(1281, 960).is_err());
    }
}
