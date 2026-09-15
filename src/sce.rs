//! 读 KovaaK 风格的 `.sce`，变成训练用的 [`ScenarioSpec`]。
//!
//! 菜单上出现哪些场景 **完全由** `res/scenarios/*.sce` 决定。文件名（不含扩展名）
//! 就是场景 id。最多列出 [`MENU_LIMIT`] 个。
//!
//! 解析策略（不必 100% 兼容 KovaaK）：
//! - `AddedBots` 用分号分隔，个数 = 同时存在的目标数
//! - Bot 的 `SpawnOffset` 跨度很大，或地图名带 gridshot → 格子模式
//! - 否则 → 墙面随机点（Sixshot 一类）
//!
//! 旧存档里的 `six-targets` / `grid-shot` 会经 [`migrate_scenario_id`] 改成现在的文件名。

use std::collections::HashMap;
use std::path::Path;

use crate::error::{Result, ShotError};
use crate::paths::AppPaths;
use crate::scenario::{GRID_RADIUS, TARGET_RADIUS};
use crate::settings::SESSION_SECS;

pub const SCENARIOS_DIR: &str = "scenarios";
pub const MENU_LIMIT: usize = 10;

/// 目标怎么刷。改玩法通常是加一个变体，然后在 [`crate::viewport`] 的 `fire_hitscan` 里处理。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnKind {
    /// 墙面上随机撒点（Sixshot）。
    RandomWall,
    /// `cells` 个格子里同时活 `live_count` 个（Gridshot 默认 9 格活 3 个）。
    Grid { cells: usize },
}

/// 一份场景的「运行时规格」。viewport 只看这个，不再读 `.sce`。
#[derive(Clone, Debug, PartialEq)]
pub struct ScenarioSpec {
    pub id: String,
    pub display_name: String,
    pub duration_secs: f32,
    pub live_count: usize,
    pub target_radius: f32,
    pub spawn: SpawnKind,
    pub glossy: bool,
}

impl ScenarioSpec {
    /// 找不到 `.sce` 时的保底，避免回放直接崩溃。
    pub fn fallback(id: &str) -> Self {
        Self {
            id: id.to_string(),
            display_name: id.to_string(),
            duration_secs: SESSION_SECS,
            live_count: 6,
            target_radius: TARGET_RADIUS,
            spawn: SpawnKind::RandomWall,
            glossy: false,
        }
    }
}

/// 历史记录和旧 `.shot` 里可能还写着枚举风格的 id。
pub fn migrate_scenario_id(raw: &str) -> String {
    match raw.trim() {
        "SixTargets" | "six-targets" | "six" | "1w6ts" => "Sixshot Ultimate".into(),
        "GridShot" | "grid-shot" | "grid" | "gridshot" => "Gridshot Ultimate".into(),
        other => other.to_string(),
    }
}

/// 扫描 `res/scenarios`，按显示名排序。坏文件会被跳过。
pub fn list_scenarios(paths: &AppPaths) -> Vec<ScenarioSpec> {
    let dir = paths.res.join(SCENARIOS_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("sce"))
        {
            continue;
        }
        if let Ok(spec) = load_scenario_file(&path) {
            items.push(spec);
        }
    }
    items.sort_by(|a, b| {
        a.display_name
            .cmp(&b.display_name)
            .then_with(|| a.id.cmp(&b.id))
    });
    items
}

/// 按 id（或旧别名）加载。找不到返回 [`ShotError::MissingFile`]。
pub fn load_scenario(paths: &AppPaths, id: &str) -> Result<ScenarioSpec> {
    let id = migrate_scenario_id(id);
    let path = paths.res.join(SCENARIOS_DIR).join(format!("{id}.sce"));
    if path.is_file() {
        return load_scenario_file(&path);
    }
    list_scenarios(paths)
        .into_iter()
        .find(|spec| {
            spec.id.eq_ignore_ascii_case(&id) || spec.display_name.eq_ignore_ascii_case(&id)
        })
        .ok_or_else(|| ShotError::MissingFile(path))
}

pub fn ensure_scenario_dir(paths: &AppPaths) -> std::io::Result<()> {
    std::fs::create_dir_all(paths.res.join(SCENARIOS_DIR))
}

fn load_scenario_file(path: &Path) -> Result<ScenarioSpec> {
    let id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("scenario")
        .to_string();
    let raw = std::fs::read_to_string(path)?;
    Ok(parse_sce(&raw, &id))
}

/// 把 INI 文本收成规格。格子判定：SpawnOffset 跨度很大，或地图名带 gridshot。
fn parse_sce(raw: &str, id: &str) -> ScenarioSpec {
    let header_end = raw.find("\n[").unwrap_or(raw.len());
    let header = parse_kv(&raw[..header_end]);
    let display_name = header
        .get("Name")
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.to_string());
    let duration_secs = header
        .get("Timelimit")
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|secs| secs.is_finite() && *secs >= 5.0)
        .unwrap_or(SESSION_SECS)
        .min(180.0);
    let bots = split_list(
        header
            .get("AddedBots")
            .or_else(|| header.get("BotCharacters")),
    );
    let live_count = bots.len().clamp(1, 16);
    let map_name = header.get("MapName").cloned().unwrap_or_default();
    let profiles = parse_profiles(raw);
    let profile = pick_bot_profile(&profiles, &bots);
    let (y_span, z_span) = profile
        .map(|profile| offset_span(profile))
        .unwrap_or((0.0, 0.0));
    let is_grid = (y_span > 100.0 && z_span > 100.0)
        || map_name.to_ascii_lowercase().contains("gridshot")
        || map_name.to_ascii_lowercase().contains("grid shot");
    let radius_cm = profile
        .and_then(|profile| profile.get("MainBBRadius"))
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|radius| radius.is_finite() && *radius > 0.0)
        .unwrap_or(if is_grid { 80.0 } else { 60.0 });
    let (target_radius, spawn, glossy) = if is_grid {
        let radius = ((radius_cm / 80.0) * GRID_RADIUS).clamp(0.12, 0.45);
        (
            radius,
            SpawnKind::Grid {
                cells: grid_cell_count(live_count),
            },
            true,
        )
    } else {
        let radius = ((radius_cm / 60.0) * TARGET_RADIUS).clamp(0.03, 0.16);
        (radius, SpawnKind::RandomWall, false)
    };
    ScenarioSpec {
        id: id.to_string(),
        display_name,
        duration_secs,
        live_count: if is_grid {
            live_count.min(grid_cell_count(live_count))
        } else {
            live_count
        },
        target_radius,
        spawn,
        glossy,
    }
}

fn grid_cell_count(live_count: usize) -> usize {
    if live_count <= 3 {
        9
    } else {
        let side = ((live_count as f32).sqrt().ceil() as usize).max(3);
        side * side
    }
}

fn split_list(raw: Option<&String>) -> Vec<String> {
    raw.map(|value| {
        value
            .split(';')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(ToString::to_string)
            .collect()
    })
    .unwrap_or_default()
}

fn parse_kv(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('[')
            || line.starts_with("//")
            || line.starts_with('#')
        {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        map.insert(key.trim().to_string(), value.trim().to_string());
    }
    map
}

fn parse_profiles(raw: &str) -> Vec<HashMap<String, String>> {
    let mut profiles = Vec::new();
    let mut current = HashMap::new();
    let mut in_profile = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('{') {
            break;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_profile && current.contains_key("Name") {
                profiles.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
            in_profile = true;
            continue;
        }
        if !in_profile {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            current.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    if in_profile && current.contains_key("Name") {
        profiles.push(current);
    }
    profiles
}

fn pick_bot_profile<'a>(
    profiles: &'a [HashMap<String, String>],
    bots: &[String],
) -> Option<&'a HashMap<String, String>> {
    let keys: Vec<String> = bots.iter().map(|name| profile_key(name)).collect();
    let mut matches: Vec<&HashMap<String, String>> = profiles
        .iter()
        .filter(|profile| {
            profile
                .get("Name")
                .is_some_and(|name| keys.iter().any(|key| *key == profile_key(name)))
        })
        .collect();
    if matches.is_empty() {
        matches = profiles
            .iter()
            .filter(|profile| {
                profile
                    .get("MainBBType")
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("Spheroid"))
            })
            .collect();
    }
    matches
        .iter()
        .rev()
        .find(|profile| {
            profile
                .get("MainBBType")
                .is_some_and(|kind| kind.eq_ignore_ascii_case("Spheroid"))
        })
        .copied()
        .or_else(|| matches.last().copied())
}

fn profile_key(name: &str) -> String {
    name.trim()
        .trim_end_matches(".bot")
        .trim()
        .to_ascii_lowercase()
}

fn offset_span(profile: &HashMap<String, String>) -> (f32, f32) {
    let min = parse_xyz(
        profile
            .get("SpawnOffsetMin")
            .map(String::as_str)
            .unwrap_or(""),
    );
    let max = parse_xyz(
        profile
            .get("SpawnOffsetMax")
            .map(String::as_str)
            .unwrap_or(""),
    );
    ((max.1 - min.1).abs(), (max.2 - min.2).abs())
}

fn parse_xyz(raw: &str) -> (f32, f32, f32) {
    let mut x = 0.0;
    let mut y = 0.0;
    let mut z = 0.0;
    for part in raw.split_whitespace() {
        if let Some(value) = part.strip_prefix("X=") {
            x = value.parse().unwrap_or(0.0);
        } else if let Some(value) = part.strip_prefix("Y=") {
            y = value.parse().unwrap_or(0.0);
        } else if let Some(value) = part.strip_prefix("Z=") {
            z = value.parse().unwrap_or(0.0);
        }
    }
    (x, y, z)
}

#[cfg(test)]
mod tests {
    use super::{
        GRID_RADIUS, SpawnKind, TARGET_RADIUS, list_scenarios, load_scenario, migrate_scenario_id,
        parse_sce,
    };
    use crate::paths::AppPaths;

    #[test]
    fn migrate_should_map_legacy_enum_ids() {
        assert_eq!(migrate_scenario_id("SixTargets"), "Sixshot Ultimate");
        assert_eq!(migrate_scenario_id("six-targets"), "Sixshot Ultimate");
        assert_eq!(migrate_scenario_id("GridShot"), "Gridshot Ultimate");
        assert_eq!(migrate_scenario_id("grid-shot"), "Gridshot Ultimate");
        assert_eq!(migrate_scenario_id("Custom Arena"), "Custom Arena");
    }

    #[test]
    fn parse_should_read_gridshot_live_count_and_grid_layout() {
        let spec = parse_sce(
            "Name=Gridshot Ultimate\nTimelimit=60.0\nAddedBots=Gridshot Bot.bot;Gridshot Bot.bot;Gridshot Bot.bot\nMapName=gridshot vertical 382 spacing.json\n\n[Bot Profile]\nName=Gridshot Bot\nMainBBType=Spheroid\nMainBBRadius=80.0\nSpawnOffsetMin=X=0.000 Y=-750.000 Z=-750.000\nSpawnOffsetMax=X=0.000 Y=750.000 Z=750.000\n",
            "Gridshot Ultimate",
        );
        assert_eq!(spec.display_name, "Gridshot Ultimate");
        assert_eq!(spec.live_count, 3);
        assert_eq!(spec.spawn, SpawnKind::Grid { cells: 9 });
        assert!(spec.glossy);
        assert!((spec.target_radius - GRID_RADIUS).abs() < 1e-4);
        assert!((spec.duration_secs - 60.0).abs() < 1e-4);
    }

    #[test]
    fn parse_should_read_sixshot_random_wall() {
        let spec = parse_sce(
            "Name=Sixshot Ultimate\nTimelimit=60.0\nAddedBots=target.bot;target.bot;target.bot;target.bot;target.bot;target.bot\nMapName=TileFrenzy_02x.json\n\n[Bot Profile]\nName=target\nMainBBType=Spheroid\nMainBBRadius=60.0\nSpawnOffsetMin=X=0.000 Y=0.000 Z=0.000\nSpawnOffsetMax=X=0.000 Y=0.000 Z=0.000\n",
            "Sixshot Ultimate",
        );
        assert_eq!(spec.live_count, 6);
        assert_eq!(spec.spawn, SpawnKind::RandomWall);
        assert!(!spec.glossy);
        assert!((spec.target_radius - TARGET_RADIUS).abs() < 1e-4);
    }

    #[test]
    fn bundled_scenario_files_should_load() {
        let paths = AppPaths::from_root(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let listed = list_scenarios(&paths);
        assert!(listed.iter().any(|spec| spec.id == "Gridshot Ultimate"));
        assert!(listed.iter().any(|spec| spec.id == "Sixshot Ultimate"));
        let grid = load_scenario(&paths, "grid-shot").unwrap();
        assert_eq!(grid.spawn, SpawnKind::Grid { cells: 9 });
        let six = load_scenario(&paths, "six-targets").unwrap();
        assert_eq!(six.spawn, SpawnKind::RandomWall);
        assert_eq!(six.live_count, 6);
    }

    #[test]
    fn list_should_sort_by_display_name() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        std::fs::create_dir_all(paths.res.join("scenarios")).unwrap();
        std::fs::write(
            paths.res.join("scenarios").join("zeta.sce"),
            "Name=Zeta\nAddedBots=a.bot\nMainBBType=Spheroid\nMainBBRadius=60.0\n",
        )
        .unwrap();
        std::fs::write(
            paths.res.join("scenarios").join("alpha.sce"),
            "Name=Alpha\nAddedBots=a.bot\nMainBBType=Spheroid\nMainBBRadius=60.0\n",
        )
        .unwrap();
        let names: Vec<_> = list_scenarios(&paths)
            .into_iter()
            .map(|spec| spec.display_name)
            .collect();
        assert_eq!(names, vec!["Alpha", "Zeta"]);
    }
}
