//! 扫描 `res/themes`、`res/sounds`、`res/crosshairs`。
//!
//! 设置里存的是相对路径，例如 `res/themes/主题.json`。旧版本把文件直接丢在 `res/`
//! 根下，[`migrate_setting_path`] 会按文件名找回新位置。
//!
//! 列表会递归子文件夹，所以你可以建 `res/sounds/kovaak/命中.ogg`。

use std::path::Path;

use crate::paths::AppPaths;

pub const THEMES_DIR: &str = "themes";
pub const SOUNDS_DIR: &str = "sounds";
pub const CROSSHAIRS_DIR: &str = "crosshairs";
pub const SCENARIOS_DIR: &str = "scenarios";

pub const DEFAULT_THEME: &str = "res/themes/主题.json";
pub const DEFAULT_CROSSHAIR: &str = "res/crosshairs/准星.png";
pub const DEFAULT_HIT_SOUND: &str = "res/sounds/命中.ogg";
pub const DEFAULT_MISS_SOUND: &str = "res/sounds/未命中.ogg";

const THEME_EXTS: &[&str] = &["json"];
const SOUND_EXTS: &[&str] = &["ogg", "wav"];
const CROSSHAIR_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// 下拉框一行：`relative` 写入 Settings，`label` 给人看。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetItem {
    pub relative: String,
    pub label: String,
}

pub fn normalize(path: &str) -> String {
    path.replace('\\', "/")
}

pub fn display_name(relative: &str) -> String {
    Path::new(relative)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(relative)
        .to_string()
}

pub fn list_themes(paths: &AppPaths) -> Vec<AssetItem> {
    list_assets(paths, THEMES_DIR, THEME_EXTS)
}

pub fn list_sounds(paths: &AppPaths) -> Vec<AssetItem> {
    list_assets(paths, SOUNDS_DIR, SOUND_EXTS)
}

pub fn list_crosshairs(paths: &AppPaths) -> Vec<AssetItem> {
    list_assets(paths, CROSSHAIRS_DIR, CROSSHAIR_EXTS)
}

pub fn migrate_setting_path(
    paths: &AppPaths,
    current: &str,
    folder: &str,
    exts: &[&str],
) -> String {
    let current = normalize(current);
    if paths.resolve_res(&current).is_file() {
        return current;
    }
    let name = Path::new(&current)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&current);
    let stem = Path::new(name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name);
    let nested = format!("res/{folder}/{name}");
    if paths.resolve_res(&nested).is_file() {
        return nested;
    }
    for ext in exts {
        let candidate = format!("res/{folder}/{stem}.{ext}");
        if paths.resolve_res(&candidate).is_file() {
            return candidate;
        }
    }
    current
}

pub fn migrate_theme_path(paths: &AppPaths, current: &str) -> String {
    migrate_setting_path(paths, current, THEMES_DIR, THEME_EXTS)
}

pub fn migrate_sound_path(paths: &AppPaths, current: &str) -> String {
    migrate_setting_path(paths, current, SOUNDS_DIR, SOUND_EXTS)
}

pub fn migrate_crosshair_path(paths: &AppPaths, current: &str) -> String {
    migrate_setting_path(paths, current, CROSSHAIRS_DIR, CROSSHAIR_EXTS)
}

fn list_assets(paths: &AppPaths, folder: &str, exts: &[&str]) -> Vec<AssetItem> {
    let root = paths.res.join(folder);
    let mut items = Vec::new();
    collect_assets(paths, &root, folder, exts, &mut items);
    items.sort_by(|a, b| {
        a.label
            .cmp(&b.label)
            .then_with(|| a.relative.cmp(&b.relative))
    });
    items
}

fn collect_assets(
    paths: &AppPaths,
    dir: &Path,
    folder: &str,
    exts: &[&str],
    items: &mut Vec<AssetItem>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_assets(paths, &path, folder, exts, items);
            continue;
        }
        if !has_ext(&path, exts) {
            continue;
        }
        let Some(relative) = relative_res_path(paths, &path) else {
            continue;
        };
        let nested = path
            .strip_prefix(paths.res.join(folder))
            .ok()
            .and_then(|rest| rest.parent())
            .filter(|parent| !parent.as_os_str().is_empty())
            .and_then(|parent| parent.to_str());
        let stem = display_name(&relative);
        let label = match nested {
            Some(parent) => format!("{parent}/{stem}"),
            None => stem,
        };
        items.push(AssetItem { relative, label });
    }
}

fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| exts.iter().any(|want| ext.eq_ignore_ascii_case(want)))
}

fn relative_res_path(paths: &AppPaths, abs: &Path) -> Option<String> {
    abs.strip_prefix(&paths.res).ok().map(|rest| {
        let rest = rest.to_string_lossy().replace('\\', "/");
        format!("res/{rest}")
    })
}

pub fn ensure_asset_dirs(paths: &AppPaths) -> std::io::Result<()> {
    std::fs::create_dir_all(paths.res.join(THEMES_DIR))?;
    std::fs::create_dir_all(paths.res.join(SOUNDS_DIR))?;
    std::fs::create_dir_all(paths.res.join(CROSSHAIRS_DIR))?;
    std::fs::create_dir_all(paths.res.join(SCENARIOS_DIR))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CROSSHAIRS_DIR, SOUNDS_DIR, THEMES_DIR, list_sounds, list_themes, migrate_setting_path,
        normalize,
    };
    use crate::paths::AppPaths;

    #[test]
    fn migrate_setting_path_should_find_nested_file_by_stem() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        std::fs::create_dir_all(paths.res.join(THEMES_DIR)).unwrap();
        std::fs::write(paths.res.join(THEMES_DIR).join("主题.json"), "{}").unwrap();
        let migrated = migrate_setting_path(&paths, "res/主题.json", THEMES_DIR, &["json"]);
        assert_eq!(migrated, "res/themes/主题.json");
    }

    #[test]
    fn list_themes_should_include_nested_json() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        std::fs::create_dir_all(paths.res.join(THEMES_DIR)).unwrap();
        std::fs::write(paths.res.join(THEMES_DIR).join("森林.json"), "{}").unwrap();
        let items = list_themes(&paths);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].relative, "res/themes/森林.json");
        assert_eq!(items[0].label, "森林");
    }

    #[test]
    fn list_sounds_should_include_pack_subfolder() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        let pack = paths.res.join(SOUNDS_DIR).join("kovaak");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(pack.join("命中.ogg"), b"ogg").unwrap();
        let items = list_sounds(&paths);
        assert_eq!(items[0].relative, "res/sounds/kovaak/命中.ogg");
        assert_eq!(items[0].label, "kovaak/命中");
    }

    #[test]
    fn normalize_should_unify_slashes() {
        assert_eq!(
            normalize(r"res\crosshairs\准星.png"),
            "res/crosshairs/准星.png"
        );
        let _ = CROSSHAIRS_DIR;
    }
}
