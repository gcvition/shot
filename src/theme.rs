//! KovaaK 主题 JSON（camelCase）。只取墙/地/天/敌人颜色，其它字段忽略。
//!
//! 文件坏了或缺失时 [`Theme::fallback`] 仍能开训，房间会变成浅灰。

use serde::Deserialize;

use crate::error::Result;
use crate::paths::AppPaths;

#[derive(Clone, Debug, Deserialize)]
pub struct ThemeColor3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ThemeRgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    #[serde(default)]
    pub a: u8,
}

/// 一份主题。字段全是 Option，缺什么就用 fallback 颜色。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    #[serde(default)]
    pub theme_name: String,
    #[serde(default)]
    pub wall_tint: Option<ThemeColor3>,
    #[serde(default)]
    pub wall_roughness: Option<f32>,
    #[serde(default)]
    pub wall_metallic: Option<f32>,
    #[serde(default)]
    pub wall_full_bright: Option<f32>,
    #[serde(default)]
    pub floor_tint: Option<ThemeColor3>,
    #[serde(default)]
    pub floor_roughness: Option<f32>,
    #[serde(default)]
    pub floor_metallic: Option<f32>,
    #[serde(default)]
    pub floor_full_bright: Option<f32>,
    #[serde(default)]
    pub ceiling_tint: Option<ThemeColor3>,
    #[serde(default)]
    pub ceiling_full_bright: Option<f32>,
    #[serde(default)]
    pub enemy_body_color: Option<ThemeColor3>,
    #[serde(default)]
    pub enemy_head_color: Option<ThemeColor3>,
    #[serde(default)]
    pub enemy_color_metallic: Option<f32>,
    #[serde(default)]
    pub enemy_color_roughness: Option<f32>,
    #[serde(default)]
    pub sky_color: Option<ThemeRgba>,
}

impl Theme {
    /// 读 JSON。文件不存在不当错误，直接用 fallback 颜色。
    pub fn load(paths: &AppPaths, relative: &str) -> Result<Self> {
        let path = paths.resolve_res(relative);
        if !path.exists() {
            return Ok(Self::fallback());
        }
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn fallback() -> Self {
        Self {
            theme_name: "fallback".into(),
            wall_tint: Some(ThemeColor3 {
                x: 0.82,
                y: 0.82,
                z: 0.84,
            }),
            wall_roughness: Some(0.4),
            wall_metallic: Some(0.1),
            wall_full_bright: Some(0.2),
            floor_tint: Some(ThemeColor3 {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            }),
            floor_roughness: Some(0.8),
            floor_metallic: Some(0.0),
            floor_full_bright: Some(0.1),
            ceiling_tint: Some(ThemeColor3 {
                x: 0.75,
                y: 0.75,
                z: 0.78,
            }),
            ceiling_full_bright: Some(0.55),
            enemy_body_color: Some(ThemeColor3 {
                x: 0.05,
                y: 0.05,
                z: 0.05,
            }),
            enemy_head_color: Some(ThemeColor3 {
                x: 0.05,
                y: 0.05,
                z: 0.05,
            }),
            enemy_color_metallic: Some(0.5),
            enemy_color_roughness: Some(0.15),
            sky_color: Some(ThemeRgba {
                r: 210,
                g: 220,
                b: 230,
                a: 255,
            }),
        }
    }

    pub fn wall_rgb(&self) -> [f32; 3] {
        tint(self.wall_tint.as_ref(), [0.8, 0.8, 0.8])
    }

    pub fn floor_rgb(&self) -> [f32; 3] {
        tint(self.floor_tint.as_ref(), [1.0, 1.0, 1.0])
    }

    pub fn ceiling_rgb(&self) -> [f32; 3] {
        tint(self.ceiling_tint.as_ref(), [0.75, 0.75, 0.78])
    }

    pub fn enemy_rgb(&self) -> [f32; 3] {
        tint(self.enemy_body_color.as_ref(), [0.02, 0.02, 0.02])
    }

    pub fn enemy_metallic(&self) -> f32 {
        self.enemy_color_metallic.unwrap_or(0.5).clamp(0.0, 1.0)
    }

    pub fn enemy_roughness(&self) -> f32 {
        self.enemy_color_roughness.unwrap_or(0.15).clamp(0.04, 1.0)
    }

    pub fn sky_rgb(&self) -> [f32; 3] {
        self.sky_color
            .as_ref()
            .map(|c| [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0])
            .unwrap_or([0.82, 0.86, 0.90])
    }

    pub fn wall_full_bright(&self) -> f32 {
        self.wall_full_bright.unwrap_or(0.55).clamp(0.0, 1.0)
    }

    pub fn floor_full_bright(&self) -> f32 {
        self.floor_full_bright.unwrap_or(0.8).clamp(0.0, 1.0)
    }

    pub fn ceiling_full_bright(&self) -> f32 {
        self.ceiling_full_bright.unwrap_or(0.55).clamp(0.0, 1.0)
    }
}

fn tint(color: Option<&ThemeColor3>, fallback: [f32; 3]) -> [f32; 3] {
    color.map(|c| [c.x, c.y, c.z]).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::Theme;
    use crate::paths::AppPaths;

    #[test]
    fn load_should_parse_bundled_kovaak_theme() {
        let paths = AppPaths::from_root(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let theme = Theme::load(&paths, crate::assets::DEFAULT_THEME).unwrap();
        assert_eq!(theme.theme_name, "clover-alternate");
        let sky = theme.sky_rgb();
        assert!((sky[0] - 1.0).abs() < 1e-5);
        assert!((theme.enemy_metallic() - 0.496).abs() < 0.02);
        assert!((theme.enemy_roughness() - 0.15).abs() < 0.02);
    }
}
