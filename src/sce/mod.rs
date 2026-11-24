//! 读 KovaaK 风格的 `.sce`，变成训练用的 [`ScenarioSpec`]。
//!
//! 菜单上出现哪些场景 **完全由** `res/scenarios/**/*.sce` 决定
//! （子目录递归，子目录名不参与 id）。文件名（不含扩展名）就是场景 id。
//!
//! 管线：文件 → [`SceDocument`]（分段）→ [`convert`] → [`ScenarioSpec`]。
//! v1 场景（没有 `[Shot Extension]`）走 `legacy` 字段归一化；
//! 带扩展段的文件叠加 M0 的标量键，DSL 键由后续里程碑接管。
//!
//! 旧存档里的 `six-targets` / `grid-shot` 会经 [`migrate_scenario_id`] 改成现在的文件名。

mod convert;
mod document;
mod extension;
mod spec;

pub use document::{KvSection, SceDocument, SceWarning};
pub use spec::{
    AdsSpec, Block, BotSpec, DifficultyKnobs, EndCondition, FireMode, LegacySpawn, MapSpec,
    MotionSpec, MovementPreset, PlayerSpec, RecoilSpec, ScenarioCategory, ScenarioSpec,
    ScenarioTags, ScoringMode, ScoringSpec, SpawnGroup, SpawnPoint, SpawnRegion, SpawnRule,
    SpreadSpec, TargetShape, UnitMode, WeaponKind, WeaponSpec,
};

use std::path::{Path, PathBuf};

use crate::error::{Result, ShotError};
use crate::paths::AppPaths;

pub const SCENARIOS_DIR: &str = "scenarios";

/// 历史记录和旧 `.shot` 里可能还写着枚举风格的 id。
pub fn migrate_scenario_id(raw: &str) -> String {
    match raw.trim() {
        "SixTargets" | "six-targets" | "six" | "1w6ts" => "Sixshot Ultimate".into(),
        "GridShot" | "grid-shot" | "grid" | "gridshot" => "Gridshot Ultimate".into(),
        other => other.to_string(),
    }
}

/// 递归扫描 `res/scenarios`，按显示名排序。坏文件会被跳过；
/// 同 id 的文件保留排序后第一个，重复的记录进返回的 warnings。
pub fn list_scenarios(paths: &AppPaths) -> Vec<ScenarioSpec> {
    list_scenarios_with_warnings(paths).0
}

/// 同 [`list_scenarios`]，但把 warnings 也带出来（启动时打印）。
pub fn list_scenarios_with_warnings(paths: &AppPaths) -> (Vec<ScenarioSpec>, Vec<SceWarning>) {
    let dir = paths.res.join(SCENARIOS_DIR);
    let mut files = Vec::new();
    collect_sce_files(&dir, &mut files);
    files.sort();
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    for path in files {
        match load_scenario_file(&path) {
            Ok((spec, file_warnings)) => {
                if items
                    .iter()
                    .any(|seen: &ScenarioSpec| seen.id.eq_ignore_ascii_case(&spec.id))
                {
                    warnings.push(SceWarning::new(format!(
                        "场景 id `{}` 重复（{}），保留先扫描到的",
                        spec.id,
                        path.display()
                    )));
                    continue;
                }
                warnings.extend(file_warnings);
                items.push(spec);
            }
            Err(err) => warnings.push(SceWarning::new(format!(
                "场景 {} 加载失败：{err}",
                path.display()
            ))),
        }
    }
    items.sort_by(|a, b| {
        a.display_name
            .cmp(&b.display_name)
            .then_with(|| a.id.cmp(&b.id))
    });
    (items, warnings)
}

fn collect_sce_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sce_files(&path, out);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("sce"))
        {
            out.push(path);
        }
    }
}

/// 按 id（或旧别名）加载。找不到返回 [`ShotError::MissingFile`]。
pub fn load_scenario(paths: &AppPaths, id: &str) -> Result<ScenarioSpec> {
    let id = migrate_scenario_id(id);
    let direct = paths.res.join(SCENARIOS_DIR).join(format!("{id}.sce"));
    if direct.is_file() {
        return Ok(load_scenario_file(&direct)?.0);
    }
    list_scenarios(paths)
        .into_iter()
        .find(|spec| {
            spec.id.eq_ignore_ascii_case(&id) || spec.display_name.eq_ignore_ascii_case(&id)
        })
        .ok_or_else(|| ShotError::MissingFile(direct))
}

/// 加载并返回 warnings（调试 / 编辑器用）。
pub fn load_scenario_with_warnings(
    paths: &AppPaths,
    id: &str,
) -> Result<(ScenarioSpec, Vec<SceWarning>)> {
    let id = migrate_scenario_id(id);
    let direct = paths.res.join(SCENARIOS_DIR).join(format!("{id}.sce"));
    if direct.is_file() {
        return load_scenario_file(&direct);
    }
    let mut files = Vec::new();
    collect_sce_files(&paths.res.join(SCENARIOS_DIR), &mut files);
    for path in files {
        if path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.eq_ignore_ascii_case(&id))
        {
            return load_scenario_file(&path);
        }
    }
    Err(ShotError::MissingFile(direct))
}

pub fn ensure_scenario_dir(paths: &AppPaths) -> std::io::Result<()> {
    std::fs::create_dir_all(paths.res.join(SCENARIOS_DIR))
}

fn load_scenario_file(path: &Path) -> Result<(ScenarioSpec, Vec<SceWarning>)> {
    let id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("scenario")
        .to_string();
    let raw = std::fs::read_to_string(path)?;
    let doc = SceDocument::parse(&raw);
    Ok(convert::convert(&doc, &id))
}

#[cfg(test)]
mod tests {
    use super::{
        LegacySpawn, SceDocument, convert::convert, list_scenarios, load_scenario,
        migrate_scenario_id,
    };
    use crate::paths::AppPaths;
    use crate::scenario::{GRID_RADIUS, TARGET_RADIUS};

    fn parse_sce(raw: &str, id: &str) -> super::ScenarioSpec {
        convert(&SceDocument::parse(raw), id).0
    }

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
        assert_eq!(spec.legacy, Some(LegacySpawn::Grid { cells: 9 }));
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
        assert_eq!(spec.legacy, Some(LegacySpawn::RandomWall));
        assert!(!spec.glossy);
        assert!((spec.target_radius - TARGET_RADIUS).abs() < 1e-4);
    }

    #[test]
    fn document_should_bucket_sections_and_stop_at_json() {
        let doc = SceDocument::parse(
            "Name=X\n\n[Bot Profile]\nName=b1\nMainBBType=Spheroid\n\n[Bot Profile]\nName=b2\n\n[Shot Extension]\nShotUnits=unreal\n\n{\"kind\":\"map\"}",
        );
        assert_eq!(doc.bots.len(), 2);
        assert_eq!(doc.bots[0].name, "b1");
        assert!(doc.shot_ext.is_some());
        assert!(doc.map_data.is_some());
        assert_eq!(doc.profiles().len(), 2);
    }

    #[test]
    fn extension_should_apply_units_and_category() {
        let (spec, warnings) = convert(
            &SceDocument::parse(
                "Name=X\nAddedBots=a.bot\n\n[Bot Profile]\nName=a\nMainBBType=Spheroid\n\n[Shot Extension]\nShotUnits=unreal\nShotCategory=Flick\nShotSkillWeights=flick=0.7;track=0.3\n",
            ),
            "X",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(spec.units, super::UnitMode::Unreal);
        assert_eq!(spec.category, Some(super::ScenarioCategory::Flick));
        assert_eq!(spec.tags.skill_weights.len(), 2);
    }

    #[test]
    fn extension_unknown_keys_should_warn_not_fail() {
        let (spec, warnings) = convert(
            &SceDocument::parse(
                "Name=X\nAddedBots=a.bot\n\n[Bot Profile]\nName=a\nMainBBType=Spheroid\n\n[Shot Extension]\nShotSpawnRegion=wall 10 10\n",
            ),
            "X",
        );
        assert_eq!(spec.id, "X");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn bundled_scenario_files_should_load() {
        let paths = AppPaths::from_root(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let listed = list_scenarios(&paths);
        assert!(listed.iter().any(|spec| spec.id == "Gridshot Ultimate"));
        assert!(listed.iter().any(|spec| spec.id == "Sixshot Ultimate"));
        let grid = load_scenario(&paths, "grid-shot").unwrap();
        assert_eq!(grid.legacy, Some(LegacySpawn::Grid { cells: 9 }));
        let six = load_scenario(&paths, "six-targets").unwrap();
        assert_eq!(six.legacy, Some(LegacySpawn::RandomWall));
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

    #[test]
    fn list_should_recurse_into_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        let nested = paths.res.join("scenarios").join("tracking");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("deep.sce"),
            "Name=Deep\nAddedBots=a.bot\nMainBBType=Spheroid\nMainBBRadius=60.0\n",
        )
        .unwrap();
        let listed = list_scenarios(&paths);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "deep");
    }

    #[test]
    fn list_should_warn_on_duplicate_ids() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        let nested = paths.res.join("scenarios").join("sub");
        std::fs::create_dir_all(&nested).unwrap();
        let body = "Name=Dup\nAddedBots=a.bot\nMainBBType=Spheroid\nMainBBRadius=60.0\n";
        std::fs::write(paths.res.join("scenarios").join("dup.sce"), body).unwrap();
        std::fs::write(nested.join("dup.sce"), body).unwrap();
        let (listed, warnings) = super::list_scenarios_with_warnings(&paths);
        assert_eq!(listed.len(), 1);
        assert_eq!(warnings.len(), 1);
    }
}
