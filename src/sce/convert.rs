//! `SceDocument` → `ScenarioSpec`。
//!
//! 这里的 legacy 归一化规则和 v1 `parse_sce` **逐条等价**（M0 行为不变）：
//! - `AddedBots` 用分号分隔，个数 = 同时存在的目标数
//! - Bot 的 `SpawnOffset` 跨度很大，或地图名带 gridshot → 格子模式
//! - 否则 → 墙面随机点（Sixshot 一类）
//!
//! 带 `[Shot Extension]` 的文件在上面结果上再叠扩展键。

use crate::scenario::{GRID_RADIUS, TARGET_RADIUS};
use crate::settings::SESSION_SECS;

use super::document::{KvSection, SceDocument, SceWarning};
use super::extension;
use super::spec::{EndCondition, LegacySpawn, ScenarioSpec, ScoringSpec, UnitMode};

/// 整份文档收成一份规格。返回值第二项是攒下的 warnings。
pub fn convert(doc: &SceDocument, id: &str) -> (ScenarioSpec, Vec<SceWarning>) {
    let mut warnings = doc.warnings.clone();
    let header = &doc.header;
    let display_name = header
        .get("Name")
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.to_string());
    let description = header
        .get("Description")
        .map(|d| d.trim().to_string())
        .unwrap_or_default();
    let duration_secs = header
        .get("Timelimit")
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|secs| secs.is_finite() && *secs >= 5.0)
        .unwrap_or(SESSION_SECS)
        .min(180.0);
    let bots = header
        .get("AddedBots")
        .or_else(|| header.get("BotCharacters"))
        .map(|value| {
            value
                .split(';')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let live_count = bots.len().clamp(1, 16);
    let map_name = header.get("MapName").cloned().unwrap_or_default();
    let profiles = doc.profiles();
    let profile = pick_bot_profile(&profiles, &bots);
    let (y_span, z_span) = profile.map(offset_span).unwrap_or((0.0, 0.0));
    let is_grid = (y_span > 100.0 && z_span > 100.0)
        || map_name.to_ascii_lowercase().contains("gridshot")
        || map_name.to_ascii_lowercase().contains("grid shot");
    let radius_cm = profile
        .and_then(|profile| profile.kv.get("MainBBRadius"))
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|radius| radius.is_finite() && *radius > 0.0)
        .unwrap_or(if is_grid { 80.0 } else { 60.0 });
    let (target_radius, legacy, glossy) = if is_grid {
        let radius = ((radius_cm / 80.0) * GRID_RADIUS).clamp(0.12, 0.45);
        (
            radius,
            LegacySpawn::Grid {
                cells: grid_cell_count(live_count),
            },
            true,
        )
    } else {
        let radius = ((radius_cm / 60.0) * TARGET_RADIUS).clamp(0.03, 0.16);
        (radius, LegacySpawn::RandomWall, false)
    };
    let mut spec = ScenarioSpec {
        id: id.to_string(),
        display_name,
        description,
        duration_secs,
        end: EndCondition::Duration,
        units: UnitMode::Legacy,
        live_count: if is_grid {
            live_count.min(grid_cell_count(live_count))
        } else {
            live_count
        },
        target_radius,
        glossy,
        legacy: Some(legacy),
        benchmark: Vec::new(),
        scoring: ScoringSpec::legacy_hits(),
        ..ScenarioSpec::fallback(id)
    };
    if let Some(ext) = &doc.shot_ext {
        let (tags, ext_warnings) = extension::apply(ext, &mut spec);
        spec.tags = tags;
        warnings.extend(ext_warnings);
        // 扩展段存在 = 这是 v2 场景；spawns 由 DSL 填充（M1），
        // 在那之前 legacy 仍兜住运行路径。
    }
    (spec, warnings)
}

fn grid_cell_count(live_count: usize) -> usize {
    if live_count <= 3 {
        9
    } else {
        let side = ((live_count as f32).sqrt().ceil() as usize).max(3);
        side * side
    }
}

fn pick_bot_profile<'a>(profiles: &[&'a KvSection], bots: &[String]) -> Option<&'a KvSection> {
    let keys: Vec<String> = bots.iter().map(|name| profile_key(name)).collect();
    let mut matches: Vec<&KvSection> = profiles
        .iter()
        .filter(|profile| {
            profile
                .get("Name")
                .is_some_and(|name| keys.iter().any(|key| *key == profile_key(name)))
        })
        .copied()
        .collect();
    if matches.is_empty() {
        matches = profiles
            .iter()
            .filter(|profile| {
                profile
                    .get("MainBBType")
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("Spheroid"))
            })
            .copied()
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

fn offset_span(profile: &KvSection) -> (f32, f32) {
    let min = profile.xyz("SpawnOffsetMin");
    let max = profile.xyz("SpawnOffsetMax");
    ((max.1 - min.1).abs(), (max.2 - min.2).abs())
}
