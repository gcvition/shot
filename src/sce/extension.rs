//! `[Shot Extension]` 段：本软件对 KovaaK `.sce` 的扩展键。
//!
//! M0 只认领「标量」键（版本、单位、计分、分类、技能权重、移动/武器预设）。
//! `ShotSpawnRegion`/`ShotMotion`/`ShotMap` 等 DSL 键由 M1+ 的
//! `spawns`/`map` 解释器接管——M0 里出现会给 warning，但不会失败。

use super::document::{KvSection, SceWarning};
use super::spec::{
    MovementPreset, ScenarioCategory, ScenarioTags, ScoringMode, ScoringSpec, UnitMode,
};

/// M0 认得的扩展键。
const KNOWN_KEYS: &[&str] = &[
    "ShotVersion",
    "ShotUnits",
    "ShotScoring",
    "ShotCategory",
    "ShotSkillWeights",
    "ShotMovementPreset",
    "ShotWeaponPreset",
];

/// 把 `[Shot Extension]` 段叠到 spec 的可调部分上。
///
/// `spec` 已经按 legacy 规则填好；这里只改扩展键点名的字段。
/// 返回 (tags 补丁, warnings)；有问题的键进 warnings 不中断。
pub fn apply(
    section: &KvSection,
    spec: &mut crate::sce::ScenarioSpec,
) -> (ScenarioTags, Vec<SceWarning>) {
    let mut warnings = Vec::new();
    let mut tags = ScenarioTags::default();
    for key in section.kv.keys() {
        if !KNOWN_KEYS.contains(&key.as_str()) {
            warnings.push(SceWarning::new(format!(
                "[Shot Extension] 暂不支持的键 `{key}`（M1+ 实现）"
            )));
        }
    }
    spec.units = match section.get("ShotUnits") {
        Some(raw) if raw.trim().eq_ignore_ascii_case("unreal") => UnitMode::Unreal,
        Some(raw) if raw.trim().eq_ignore_ascii_case("legacy") => UnitMode::Legacy,
        Some(raw) => {
            warnings.push(SceWarning::new(format!("ShotUnits=`{raw}` 无效，按 legacy 处理")));
            UnitMode::Legacy
        }
        None => UnitMode::Unreal, // 出现扩展段 = 显式 v2 场景，默认 unreal
    };
    if let Some(raw) = section.get("ShotScoring") {
        match raw.trim().to_ascii_lowercase().as_str() {
            "legacyhits" | "legacy" => spec.scoring = ScoringSpec::legacy_hits(),
            "kovaak" | "kovaaks" => spec.scoring.mode = ScoringMode::Kovaak,
            "ontarget" | "ontargetpercent" => spec.scoring.mode = ScoringMode::OnTargetPercent,
            "reaction" | "reactionms" => spec.scoring.mode = ScoringMode::ReactionMs,
            other => warnings.push(SceWarning::new(format!("ShotScoring=`{other}` 无效"))),
        }
    }
    if let Some(raw) = section.get("ShotCategory") {
        match ScenarioCategory::parse(raw) {
            Some(cat) => spec.category = Some(cat),
            None => warnings.push(SceWarning::new(format!("ShotCategory=`{raw}` 无效"))),
        }
    }
    if let Some(raw) = section.get("ShotSkillWeights") {
        // "flick=0.6;tracking=0.4"
        for part in raw.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let Some((skill, weight)) = part.split_once('=') else {
                warnings.push(SceWarning::new(format!(
                    "ShotSkillWeights 条目 `{part}` 缺少 `=`"
                )));
                continue;
            };
            match weight.trim().parse::<f32>() {
                Ok(w) if w.is_finite() => {
                    tags.skill_weights.push((skill.trim().to_string(), w))
                }
                _ => warnings.push(SceWarning::new(format!(
                    "ShotSkillWeights 条目 `{part}` 权重非法"
                ))),
            }
        }
    }
    if let Some(raw) = section.get("ShotMovementPreset") {
        match MovementPreset::parse(raw) {
            Some(preset) => spec.player.movement_preset = preset,
            None => warnings.push(SceWarning::new(format!(
                "ShotMovementPreset=`{raw}` 无效"
            ))),
        }
    }
    // ShotWeaponPreset 已认领但 M2 武器库才生效，这里不动作也不告警。
    (tags, warnings)
}
