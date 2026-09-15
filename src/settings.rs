//! 用户设置，落盘 `cache/settings.json`。
//!
//! 菜单改的每一项最终都写进这里，Bevy 启动时再读一遍。
//! 训练中途改设置 **不会** 热更新，要等下一局。
//!
//! [`countdown_secs`](Settings::countdown_secs) 默认 0（立刻开始），最大 30。
//! 分辨率必须是偶数，因为渲染目标按 2 字节对齐更省事。

use crate::error::{Result, ShotError};
use crate::fov::FovKind;
use crate::paths::AppPaths;
use crate::sensitivity::{DEFAULT_CM_PER_360, DEFAULT_DPI};
use serde::{Deserialize, Serialize};

/// 一局默认时长（秒）。`.sce` 里的 Timelimit 可以覆盖，但菜单文案按 60 秒写。
pub const SESSION_SECS: f32 = 60.0;

/// 训练窗口怎么铺到屏幕上。下一局才生效。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayMode {
    /// 无边框铺满主显示器，RT 会被拉伸。
    #[default]
    BorderlessStretch,
    /// 普通窗口，大小等于渲染分辨率。
    Windowed,
}

/// 菜单和 Bevy 共用的设置。缺字段时用 [`Default`]（`#[serde(default)]`）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 鼠标垫上转一圈要滑多少厘米。越小越灵敏。
    pub cm_per_360: f64,
    /// 鼠标 DPI。和 cm/360 一起决定每一 count 转多少度。
    pub dpi: u32,
    pub fov_deg: f32,
    pub fov_kind: FovKind,
    /// 3D 画到多大的 RT。可以和窗口大小不同。
    pub render_width: u32,
    pub render_height: u32,
    pub display_mode: DisplayMode,
    pub theme_path: String,
    pub crosshair_path: String,
    pub hit_sound: String,
    pub miss_sound: String,
    pub master_volume: f32,
    /// 开局倒计时秒数。0 = 立刻能开枪。
    pub countdown_secs: u32,
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
            theme_path: crate::assets::DEFAULT_THEME.into(),
            crosshair_path: crate::assets::DEFAULT_CROSSHAIR.into(),
            hit_sound: crate::assets::DEFAULT_HIT_SOUND.into(),
            miss_sound: crate::assets::DEFAULT_MISS_SOUND.into(),
            master_volume: 1.0,
            countdown_secs: 0,
        }
    }
}

impl Settings {
    /// 没有文件就按当前显示器分辨率写一份默认设置。
    pub fn load_or_default(paths: &AppPaths) -> Result<Self> {
        if paths.settings.exists() {
            let raw = std::fs::read_to_string(&paths.settings)?;
            let mut settings: Self = serde_json::from_str(&raw)?;
            settings.migrate_asset_paths(paths);
            settings.validate()?;
            Ok(settings)
        } else {
            let mut settings = Self::default();
            if let Some((width, height)) = crate::display::preferred_resolution() {
                settings.render_width = width;
                settings.render_height = height;
            }
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

    fn migrate_asset_paths(&mut self, paths: &AppPaths) {
        self.theme_path = crate::assets::migrate_theme_path(paths, &self.theme_path);
        self.crosshair_path = crate::assets::migrate_crosshair_path(paths, &self.crosshair_path);
        self.hit_sound = crate::assets::migrate_sound_path(paths, &self.hit_sound);
        self.miss_sound = crate::assets::migrate_sound_path(paths, &self.miss_sound);
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
        if self.countdown_secs > 30 {
            return Err(ShotError::Other("countdown must be 0..=30".into()));
        }
        Ok(())
    }

    pub fn render_aspect(&self) -> f32 {
        self.render_width as f32 / self.render_height as f32
    }
}

/// 宽高必须偶数，且落在 640×480 到 7680×4320。
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
    use super::{Settings, validate_resolution};

    #[test]
    fn default_settings_should_be_valid() {
        Settings::default().validate().unwrap();
    }

    #[test]
    fn odd_resolution_should_be_rejected() {
        assert!(validate_resolution(1281, 960).is_err());
    }

    #[test]
    fn load_should_ignore_removed_video_enabled_field() {
        let json = r#"{
            "cm_per_360": 19.05,
            "dpi": 800,
            "fov_deg": 103.0,
            "fov_kind": "HorizontalRes",
            "render_width": 1920,
            "render_height": 1080,
            "display_mode": "BorderlessStretch",
            "theme_path": "res/themes/主题.json",
            "crosshair_path": "res/crosshairs/准星.png",
            "hit_sound": "res/sounds/命中.ogg",
            "miss_sound": "res/sounds/未命中.ogg",
            "master_volume": 1.0,
            "video_enabled": true
        }"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        settings.validate().unwrap();
        assert!((settings.fov_deg - 103.0).abs() < 1e-4);
        assert_eq!(settings.countdown_secs, 0);
    }

    #[test]
    fn countdown_above_thirty_should_be_rejected() {
        let mut settings = Settings::default();
        settings.countdown_secs = 31;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn load_or_default_should_migrate_old_res_layout() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        std::fs::create_dir_all(paths.res.join("themes")).unwrap();
        std::fs::create_dir_all(paths.res.join("sounds")).unwrap();
        std::fs::create_dir_all(paths.res.join("crosshairs")).unwrap();
        std::fs::write(paths.res.join("themes").join("主题.json"), "{}").unwrap();
        std::fs::write(paths.res.join("sounds").join("命中.ogg"), b"ogg").unwrap();
        std::fs::write(paths.res.join("sounds").join("未命中.ogg"), b"ogg").unwrap();
        std::fs::write(paths.res.join("crosshairs").join("准星.png"), b"png").unwrap();
        let json = r#"{
            "cm_per_360": 19.05,
            "dpi": 800,
            "fov_deg": 103.0,
            "fov_kind": "HorizontalRes",
            "render_width": 1920,
            "render_height": 1080,
            "display_mode": "BorderlessStretch",
            "theme_path": "res/主题.json",
            "crosshair_path": "res/准星.png",
            "hit_sound": "res/命中.wav",
            "miss_sound": "res/未命中.wav",
            "master_volume": 1.0
        }"#;
        std::fs::write(&paths.settings, json).unwrap();
        let settings = Settings::load_or_default(&paths).unwrap();
        assert_eq!(settings.theme_path, "res/themes/主题.json");
        assert_eq!(settings.crosshair_path, "res/crosshairs/准星.png");
        assert_eq!(settings.hit_sound, "res/sounds/命中.ogg");
        assert_eq!(settings.miss_sound, "res/sounds/未命中.ogg");
    }
}
