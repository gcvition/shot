//! `[Shot Extension]` 段：本软件对 KovaaK `.sce` 的扩展键。
//!
//! M1 起认领 spawn DSL 键（`ShotSpawnRegion[N]`/`ShotMotion[N]`/
//! `ShotSpawnRule[N]`/`ShotSpawnCount[N]`/`ShotSpawnRadius[N]`/
//! `ShotSpawnShape[N]`/`ShotNoShoot[N]`，见 [`super::dsl`]）。
//! 解析出至少一组时 `spec.legacy` 被清掉——v2 场景走 spawn 导演路径。

use super::document::{KvSection, SceWarning};
use super::dsl;
use super::spec::{
    EndCondition, MovementPreset, ScenarioCategory, ScenarioTags, ScoringMode, ScoringSpec,
    UnitMode,
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
    "ShotWeapon",
    "ShotPlayer",
    "ShotMapBounds",
    "ShotBenchmark",
    "ShotEndCondition",
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
        if !KNOWN_KEYS.contains(&key.as_str()) && !dsl::is_spawn_key(key) {
            warnings.push(SceWarning::new(format!(
                "[Shot Extension] 暂不支持的键 `{key}`（后续里程碑实现）"
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
    // ShotWeapon / ShotWeaponPreset → 武器规格（M2）。
    for key in ["ShotWeapon", "ShotWeaponPreset"] {
        if let Some(raw) = section.get(key) {
            match dsl::parse_weapon(raw) {
                Ok(w) => spec.weapon = w,
                Err(e) => warnings.push(SceWarning::new(format!("{key}: {e}"))),
            }
        }
    }
    // ShotPlayer → 出生点 / 移动预设 / 血量（M4）。
    if let Some(raw) = section.get("ShotPlayer") {
        match dsl::parse_player(raw) {
            Ok(p) => spec.player = p,
            Err(e) => warnings.push(SceWarning::new(format!("ShotPlayer: {e}"))),
        }
    }
    // ShotMapBounds + ShotBlock[N] → 地图（M4）。
    if let Some(raw) = section.get("ShotMapBounds") {
        match dsl::parse_block(raw) {
            Ok(b) => spec.map.bounds = Some((b.min, b.max)),
            Err(e) => warnings.push(SceWarning::new(format!("ShotMapBounds: {e}"))),
        }
    }
    for key in section.kv.keys() {
        if let Some(raw) = section.get(key)
            && key.to_ascii_lowercase().starts_with("shotblock")
        {
            match dsl::parse_block(raw) {
                Ok(b) => spec.map.blocks.push(b),
                Err(e) => warnings.push(SceWarning::new(format!("{key}: {e}"))),
            }
        }
    }
    // ShotBenchmark=bronze=50 silver=70 gold=90（M7）。
    if let Some(raw) = section.get("ShotBenchmark") {
        match dsl::parse_benchmark(raw) {
            Ok(b) => spec.benchmark = b,
            Err(e) => warnings.push(SceWarning::new(format!("ShotBenchmark: {e}"))),
        }
    }
    // ShotEndCondition=kills:20 / damage:5000 / rounds:5 / duration:60
    if let Some(raw) = section.get("ShotEndCondition") {
        let (k, v) = raw.split_once(':').unwrap_or((raw.trim(), ""));
        let n: f32 = v.parse().unwrap_or(0.0);
        spec.end = match k.trim().to_ascii_lowercase().as_str() {
            "duration" | "time" if n > 0.0 => EndCondition::Duration,
            "kills" if n > 0.0 => EndCondition::Kills(n as u32),
            "damage" if n > 0.0 => EndCondition::Damage(n),
            "rounds" if n > 0.0 => EndCondition::Rounds(n as u32),
            _ => {
                warnings.push(SceWarning::new(format!("ShotEndCondition= 无效")));
                spec.end
            }
        };
        if n > 0.0 && matches!(spec.end, EndCondition::Duration) {
            spec.duration_secs = n;
        }
    }
    let groups = dsl::collect_groups(section, &mut warnings);
    if !groups.is_empty() {
        // v2 场景：spawn 导演接管，旧版生成路径下线。
        spec.spawns = groups;
        spec.legacy = None;
    }
    (tags, warnings)
}
