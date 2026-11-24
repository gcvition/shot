//! `[Shot Extension]` 的 DSL 键解析（M1）：
//! `ShotSpawnRegion` / `ShotSpawnRule` / `ShotMotion` / `ShotSpawnCount` /
//! `ShotSpawnRadius` / `ShotSpawnShape` / `ShotNoShoot`。
//!
//! 语法：`Kind k=v k2=v2 ...`，`v` 可为标量、`a..b` 区间、`(x,y,z)` 向量。
//! 多出生组用数字后缀：`ShotSpawnRegion2` / `ShotMotion2` / …（无后缀 = 组 1）。
//! `ShotMotion` 支持 `A + B` 复合：每段是独立 `Kind k=v ...`。

use super::document::{KvSection, SceWarning};
use super::spec::{
    Block, BotParams, DodgeSpec, FireMode, MovementPreset, MotionSpec, PlayerSpec,
    SpawnGroup, SpawnPoint, SpawnRegion, SpawnRule, SpreadSpec, TargetShape, WeaponKind, WeaponSpec,
};
use crate::vec3::Vec3;
use std::collections::HashMap;

const MAX_GROUPS: usize = 8;

/// `a..b` 或标量 → 区间。标量 = (v, v)。
fn parse_range(raw: &str) -> Option<(f32, f32)> {
    let raw = raw.trim();
    if let Some((a, b)) = raw.split_once("..") {
        let lo = a.trim().parse::<f32>().ok()?;
        let hi = b.trim().parse::<f32>().ok()?;
        (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
    } else {
        let v = raw.parse::<f32>().ok()?;
        v.is_finite().then_some((v, v))
    }
}

fn parse_f32(raw: &str) -> Option<f32> {
    let v = raw.trim().parse::<f32>().ok()?;
    v.is_finite().then_some(v)
}

/// `(x,y,z)` / `x,y,z` / `(X=x Y=y Z=z)` / `x y z`。
fn parse_vec3(raw: &str) -> Option<Vec3> {
    let raw = raw.trim().trim_start_matches('(').trim_end_matches(')');
    if raw.contains('=') {
        // KovaaK 风格 "X=1 Y=2 Z=3"
        let mut v = Vec3::ZERO;
        for part in raw.split_whitespace() {
            let (k, val) = part.split_once('=')?;
            let f = parse_f32(val)?;
            match k.trim().to_ascii_lowercase().as_str() {
                "x" => v.x = f,
                "y" => v.y = f,
                "z" => v.z = f,
                _ => return None,
            }
        }
        return Some(v);
    }
    let parts: Vec<&str> = raw
        .split([',', ' '])
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() != 3 {
        return None;
    }
    Some(Vec3::new(
        parse_f32(parts[0])?,
        parse_f32(parts[1])?,
        parse_f32(parts[2])?,
    ))
}

/// 把 `k=v k2=v2 ...`（第一个 token 是 kind）拆成 (kind, map)。
/// 键统一小写——DSL 里 `minSep`/`jumpI` 这类驼峰写法都能匹配。
fn parse_kv(raw: &str) -> (String, HashMap<String, String>) {
    let mut tokens = raw.split_whitespace().peekable();
    // 首 token 带 '=' 就不是 kind（ShotPlayer/ShotBot 没有 kind 位）。
    let kind = if tokens.peek().is_some_and(|t| t.contains('=')) {
        String::new()
    } else {
        tokens.next().unwrap_or("").to_string()
    };
    let mut map = HashMap::new();
    for tok in tokens {
        if let Some((k, v)) = tok.split_once('=') {
            map.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    (kind, map)
}

fn get_range(map: &HashMap<String, String>, key: &str) -> Option<(f32, f32)> {
    map.get(key).and_then(|v| parse_range(v))
}

fn get_f32(map: &HashMap<String, String>, key: &str) -> Option<f32> {
    map.get(key).and_then(|v| parse_f32(v))
}

fn get_vec3(map: &HashMap<String, String>, key: &str) -> Option<Vec3> {
    map.get(key).and_then(|v| parse_vec3(v))
}

fn get_bool(map: &HashMap<String, String>, key: &str) -> bool {
    matches!(
        map.get(key).map(|v| v.trim().to_ascii_lowercase()),
        Some(v) if v == "1" || v == "true" || v == "yes" || v == "on"
    )
}

/// 解析 `ShotMotion=`。支持 `KindA k=v + KindB k=v` 复合。
pub fn parse_motion(raw: &str) -> Result<MotionSpec, String> {
    let composite = raw.char_indices().any(|(i, c)| {
        c == '+'
            && raw[i + 1..]
                .chars()
                .find(|n| !n.is_whitespace())
                .is_some_and(|n| n.is_ascii_alphabetic())
    });
    if composite {
        let parts: Result<Vec<_>, _> = raw
            .split('+')
            .map(|seg| parse_motion_single(seg.trim()))
            .collect();
        return Ok(MotionSpec::Composite(parts?));
    }
    parse_motion_single(raw.trim())
}

fn parse_motion_single(raw: &str) -> Result<MotionSpec, String> {
    let (kind, m) = parse_kv(raw);
    let err = |what: &str| format!("ShotMotion={kind} 缺/坏参数 {what}（raw: `{raw}`）");
    match kind.to_ascii_lowercase().as_str() {
        "static" => Ok(MotionSpec::Static),
        "linear" => Ok(MotionSpec::Linear {
            speed: get_range(&m, "speed").ok_or_else(|| err("speed"))?,
            dir_deg: get_range(&m, "dir").unwrap_or((0.0, 360.0)),
        }),
        "strafe" => Ok(MotionSpec::Strafe {
            speed: get_range(&m, "speed").ok_or_else(|| err("speed"))?,
            interval: get_range(&m, "reverse")
                .or_else(|| get_range(&m, "interval"))
                .unwrap_or((0.5, 1.5)),
            accel: get_f32(&m, "accel").unwrap_or(30.0),
            symmetric: get_bool(&m, "symmetric"),
        }),
        "sine" => Ok(MotionSpec::Sine {
            amp: get_vec3(&m, "amp").ok_or_else(|| err("amp"))?,
            freq: get_vec3(&m, "freq").unwrap_or(Vec3::new(0.5, 0.5, 0.0)),
            phase: get_vec3(&m, "phase").unwrap_or(Vec3::ZERO),
        }),
        "orbit" => Ok(MotionSpec::Orbit {
            radius: get_f32(&m, "radius").ok_or_else(|| err("radius"))?,
            angular_speed: get_f32(&m, "speed").unwrap_or(1.0),
            center: get_vec3(&m, "center"),
            y_offset: get_f32(&m, "y").unwrap_or(0.0),
        }),
        "smoothrandom" | "smooth" => Ok(MotionSpec::SmoothRandom {
            max_speed: get_f32(&m, "speed").ok_or_else(|| err("speed"))?,
            tau: get_f32(&m, "tau").unwrap_or(0.35),
        }),
        "dodge" => {
            let mut d = DodgeSpec::default();
            if let Some(v) = get_range(&m, "speed") {
                d.speed = (v.0 + v.1) * 0.5;
            }
            if let Some(v) = get_f32(&m, "accel") {
                d.accel = v;
            }
            if let Some(v) = get_f32(&m, "gravity") {
                d.gravity = v;
            }
            if let Some(v) = get_f32(&m, "jump") {
                d.jump_speed = v;
            }
            if let Some(v) = get_range(&m, "lr") {
                d.lr_interval = v;
            }
            if let Some(v) = get_range(&m, "fb") {
                d.fb_interval = v;
            }
            if let Some(v) = get_range(&m, "jumpi") {
                d.jump_interval = v;
            }
            if let Some(v) = get_range(&m, "band") {
                d.dist_band = v;
            }
            if let Some(v) = get_f32(&m, "react") {
                d.react_chance = v;
            }
            if let Some(v) = get_range(&m, "reactdelay") {
                d.react_delay = v;
            }
            if let Some(v) = get_f32(&m, "reactcd") {
                d.react_cooldown = v;
            }
            Ok(MotionSpec::Dodge(d))
        }
        "jumper" | "jump" => Ok(MotionSpec::Jumper {
            jump_speed: get_range(&m, "speed")
                .map(|(a, b)| (a + b) * 0.5)
                .unwrap_or(6.0),
            gravity: get_f32(&m, "gravity").unwrap_or(20.0),
            interval: get_range(&m, "interval").unwrap_or((1.0, 3.0)),
        }),
        "blink" => Ok(MotionSpec::Blink {
            interval: get_range(&m, "interval").ok_or_else(|| err("interval"))?,
            dist: get_range(&m, "dist").unwrap_or((1.0, 3.0)),
        }),
        "path" => {
            // points=(x,y,z);(x,y,z);…
            let pts_raw = m.get("points").ok_or_else(|| err("points"))?;
            let points: Result<Vec<_>, _> = pts_raw
                .split(';')
                .map(|p| parse_vec3(p).ok_or_else(|| err("points")))
                .collect();
            Ok(MotionSpec::Path {
                points: points?,
                speed: get_f32(&m, "speed").unwrap_or(2.0),
                closed: get_bool(&m, "closed"),
            })
        }
        "distance" => Ok(MotionSpec::Distance {
            band: get_range(&m, "band").ok_or_else(|| err("band"))?,
            speed: get_f32(&m, "speed").unwrap_or(3.0),
        }),
        other => Err(format!("ShotMotion 未知类型 `{other}`")),
    }
}

/// 解析 `ShotSpawnRegion=`。
pub fn parse_region(raw: &str) -> Result<SpawnRegion, String> {
    let (kind, m) = parse_kv(raw);
    let err = |what: &str| format!("ShotSpawnRegion={kind} 缺/坏参数 {what}（raw: `{raw}`）");
    match kind.to_ascii_lowercase().as_str() {
        "wallrect" => Ok(SpawnRegion::WallRect {
            center: get_vec3(&m, "center").ok_or_else(|| err("center"))?,
            size: get_vec3(&m, "size").ok_or_else(|| err("size"))?,
        }),
        "grid" => Ok(SpawnRegion::Grid {
            origin: get_vec3(&m, "origin").ok_or_else(|| err("origin"))?,
            cols: get_f32(&m, "cols").unwrap_or(3.0).max(1.0) as u32,
            rows: get_f32(&m, "rows").unwrap_or(3.0).max(1.0) as u32,
            spacing: get_vec3(&m, "spacing").ok_or_else(|| err("spacing"))?,
        }),
        "box" => Ok(SpawnRegion::Box {
            min: get_vec3(&m, "min").ok_or_else(|| err("min"))?,
            max: get_vec3(&m, "max").ok_or_else(|| err("max"))?,
        }),
        "angularshell" | "shell" => Ok(SpawnRegion::AngularShell {
            yaw_range: get_range(&m, "yaw").ok_or_else(|| err("yaw"))?,
            pitch_range: get_range(&m, "pitch").unwrap_or((-10.0, 25.0)),
            dist_range: get_range(&m, "dist").ok_or_else(|| err("dist"))?,
        }),
        "points" => {
            // p=(x,y,z,w);p=(…)；w = 权重
            let mut points = Vec::new();
            for seg in raw.split(';').skip(1) {
                let seg = seg.rsplit('=').next().unwrap_or(seg).trim();
                if seg.is_empty() {
                    continue;
                }
                let v = parse_vec3(seg).ok_or_else(|| err("p"))?;
                points.push(SpawnPoint {
                    pos: Vec3::new(v.x, v.y, v.z),
                    yaw_deg: 0.0,
                    weight: 1.0,
                });
            }
            // 支持 p0=(..) p1=(..) 写法
            for (k, v) in &m {
                if k.starts_with('p')
                    && let Some(vec) = parse_vec3(v)
                {
                    points.push(SpawnPoint {
                        pos: vec,
                        yaw_deg: 0.0,
                        weight: 1.0,
                    });
                }
            }
            if points.is_empty() {
                return Err(err("p0.."));
            }
            Ok(SpawnRegion::Points(points))
        }
        other => Err(format!("ShotSpawnRegion 未知类型 `{other}`")),
    }
}

/// 解析 `ShotSpawnRule=`（无 kind 头，直接 `k=v`）。
pub fn parse_rule(raw: &str) -> Result<SpawnRule, String> {
    let mut map = HashMap::new();
    for tok in raw.split_whitespace() {
        if let Some((k, v)) = tok.split_once('=') {
            map.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let mut rule = SpawnRule::default();
    if let Some(v) = get_f32(&map, "minsep") {
        rule.min_separation = v;
    }
    if let Some(v) = get_f32(&map, "minang") {
        rule.min_angle_from_crosshair_deg = v;
    }
    if let Some(v) = get_f32(&map, "maxang") {
        rule.max_angle_from_crosshair_deg = v;
    }
    if let Some(v) = get_range(&map, "delay") {
        rule.delay = v;
    }
    rule.lifetime = get_range(&map, "life");
    if let Some(v) = get_f32(&map, "maxalive") {
        rule.max_alive = v.max(1.0) as usize;
    }
    rule.require_los = get_bool(&map, "los");
    Ok(rule)
}

/// 收集 `ShotSpawnRegion[N]` 系列键成 [`SpawnGroup`] 列表。
/// 组号：无后缀 = 1，`ShotSpawnRegion2` = 2 … 配套键 `ShotRule`/`ShotMotion` 等同样带后缀。
pub fn collect_groups(section: &KvSection, warnings: &mut Vec<SceWarning>) -> Vec<SpawnGroup> {
    let mut groups = Vec::new();
    for i in 1..=MAX_GROUPS {
        let suffix = if i == 1 {
            String::new()
        } else {
            i.to_string()
        };
        let key = format!("ShotSpawnRegion{suffix}");
        let Some(raw) = section.get(&key) else {
            continue;
        };
        let region = match parse_region(raw) {
            Ok(r) => r,
            Err(e) => {
                warnings.push(SceWarning::new(e));
                continue;
            }
        };
        let motion = match section.get(&format!("ShotMotion{suffix}")) {
            Some(raw) => match parse_motion(raw) {
                Ok(m) => m,
                Err(e) => {
                    warnings.push(SceWarning::new(e));
                    MotionSpec::Static
                }
            },
            None => MotionSpec::Static,
        };
        let rule = match section.get(&format!("ShotSpawnRule{suffix}")) {
            Some(raw) => match parse_rule(raw) {
                Ok(r) => r,
                Err(e) => {
                    warnings.push(SceWarning::new(e));
                    SpawnRule::default()
                }
            },
            None => SpawnRule::default(),
        };
        let archetype = match section
            .get(&format!("ShotSpawnShape{suffix}"))
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("humanoid") => TargetShape::Humanoid,
            Some("box") | Some("cube") => TargetShape::Box,
            Some("disc") => TargetShape::Disc,
            _ => TargetShape::Sphere,
        };
        groups.push(SpawnGroup {
            archetype,
            radius: section
                .get(&format!("ShotSpawnRadius{suffix}"))
                .and_then(parse_f32)
                .unwrap_or(0.0),
            no_shoot: section
                .get(&format!("ShotNoShoot{suffix}"))
                .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
                .unwrap_or(false),
            hp: section
                .get(&format!("ShotSpawnHP{suffix}"))
                .and_then(parse_f32)
                .unwrap_or(0.0),
            headshot_only: section
                .get(&format!("ShotHeadshotOnly{suffix}"))
                .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
                .unwrap_or(false),
            bot: section
                .get(&format!("ShotBot{suffix}"))
                .and_then(|raw| parse_bot(raw).ok()),
            count: section
                .get(&format!("ShotSpawnCount{suffix}"))
                .and_then(parse_f32)
                .unwrap_or(3.0)
                .max(1.0) as usize,
            region,
            motion,
            rule,
        });
    }
    groups
}

/// M1 认领的 spawn DSL 键前缀（`apply` 里对未知键告警时跳过这些）。
pub fn is_spawn_key(key: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "ShotSpawnRegion",
        "ShotSpawnHP",
        "ShotHeadshotOnly",
        "ShotBot",
        "ShotSpawnRule",
        "ShotMotion",
        "ShotSpawnCount",
        "ShotSpawnRadius",
        "ShotSpawnShape",
        "ShotNoShoot",
    ];
    PREFIXES.iter().any(|p| key.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_strafe_should_parse() {
        let m = parse_motion("Strafe speed=3..5 reverse=0.2..0.6 accel=30").unwrap();
        assert_eq!(
            m,
            MotionSpec::Strafe {
                speed: (3.0, 5.0),
                interval: (0.2, 0.6),
                accel: 30.0,
                symmetric: false,
            }
        );
    }

    #[test]
    fn region_shell_should_parse() {
        let r =
            parse_region("AngularShell yaw=-180..180 pitch=-10..25 dist=8..15").unwrap();
        assert_eq!(
            r,
            SpawnRegion::AngularShell {
                yaw_range: (-180.0, 180.0),
                pitch_range: (-10.0, 25.0),
                dist_range: (8.0, 15.0),
            }
        );
    }

    #[test]
    fn rule_should_parse() {
        let r = parse_rule("minSep=0.8 minAng=10 maxAng=60 delay=0.4..1.2 life=1.5").unwrap();
        assert_eq!(r.min_separation, 0.8);
        assert_eq!(r.min_angle_from_crosshair_deg, 10.0);
        assert_eq!(r.max_angle_from_crosshair_deg, 60.0);
        assert_eq!(r.delay, (0.4, 1.2));
        assert_eq!(r.lifetime, Some((1.5, 1.5)));
    }

    #[test]
    fn motion_composite_should_parse() {
        let m = parse_motion("Strafe speed=3 + Jumper interval=1..2").unwrap();
        match m {
            MotionSpec::Composite(parts) => assert_eq!(parts.len(), 2),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bad_kind_should_error() {
        assert!(parse_motion("Warp speed=3").is_err());
        assert!(parse_region("Triangle x=1").is_err());
    }
}

fn get_u32(map: &HashMap<String, String>, key: &str) -> Option<u32> {
    map.get(key)?.trim().parse::<u32>().ok()
}

/// 
pub fn parse_bot(raw: &str) -> Result<BotParams, String> {
    let (_head, map) = parse_kv(raw);
    Ok(BotParams {
        aim_time: get_range(&map, "aim").unwrap_or((0.35, 0.8)),
        fire_interval: get_f32(&map, "interval").unwrap_or(1.1),
        accuracy: get_f32(&map, "acc").unwrap_or(0.45),
        damage: get_f32(&map, "dmg").unwrap_or(8.0),
        strafe: get_bool(&map, "strafe"),
    })
}

/// 。预设：hitscan/rifle/burst/projectile/lob/beam。
pub fn parse_weapon(raw: &str) -> Result<WeaponSpec, String> {
    let (head, map) = parse_kv(raw);
    let mut w = match head.as_str() {
        "" | "hitscan" => WeaponSpec::hitscan_click(),
        "rifle" | "rifle_auto" | "auto" => WeaponSpec::rifle_auto(),
        "burst" | "rifle_burst" => WeaponSpec::rifle_burst(),
        "projectile" | "arc" => WeaponSpec::projectile_arc(),
        "lob" => WeaponSpec::projectile_lob(),
        "beam" => WeaponSpec::beam_dps(),
        other => return Err(format!("unknown weapon preset '{other}'")),
    };
    if let Some(mode) = map.get("mode") {
        w.fire_mode = match mode.trim().to_ascii_lowercase().as_str() {
            "semi" => FireMode::Semi,
            "auto" => FireMode::Auto,
            "burst" => FireMode::Burst {
                count: get_u32(&map, "burstn").unwrap_or(3),
                interval: get_f32(&map, "burstint").unwrap_or(0.07),
            },
            other => return Err(format!("unknown fire mode '{other}'")),
        };
    }
    if let Some(v) = get_f32(&map, "rpm") {
        if v > 0.0 {
            w.fire_interval = 60.0 / v;
        }
    }
    if let Some(v) = get_f32(&map, "dmg") {
        w.damage = v;
    }
    if let Some(v) = get_f32(&map, "hsmult") {
        w.headshot_mult = v;
    }
    if let Some(v) = get_u32(&map, "pellets") {
        w.pellets = v.max(1);
    }
    if let Some(v) = get_u32(&map, "mag") {
        w.magazine = if v == 0 { None } else { Some(v) };
    }
    if let Some(v) = get_f32(&map, "reload") {
        w.reload_empty = v;
        w.reload_partial = v * 0.8;
    }
    if let Some(v) = map.get("spread") {
        // spread=base[,per_shot,decay,max]
        let nums: Vec<f32> = v.split(',').filter_map(|t| t.trim().parse().ok()).collect();
        w.spread = match nums.len() {
            1 => SpreadSpec { base: nums[0], per_shot: 0.0, decay: 0.0, max: 0.0 },
            4 => SpreadSpec { base: nums[0], per_shot: nums[1], decay: nums[2], max: nums[3] },
            _ => return Err(format!("spread needs 1 or 4 numbers, got '{v}'")),
        };
    }
    if let Some(v) = map.get("recoil") {
        // recoil=pitch[,yaw]
        let nums: Vec<f32> = v.split(',').filter_map(|t| t.trim().parse().ok()).collect();
        match nums.len() {
            1 => {
                w.recoil.pitch = nums[0];
            }
            2 => {
                w.recoil.pitch = nums[0];
                w.recoil.yaw = nums[1];
            }
            _ => return Err(format!("recoil needs 1 or 2 numbers, got '{v}'")),
        }
    }
    if let Some(v) = get_f32(&map, "reset") {
        w.recoil.reset_dps = v;
    }
    if map.contains_key("negatable") {
        w.recoil.negatable = get_bool(&map, "negatable");
    }
    if let Some(v) = map.get("ads") {
        // ads=zoomfov[,sens_mult]（M8 开镜）
        let nums: Vec<f32> = v.split(',').filter_map(|t| t.trim().parse().ok()).collect();
        w.ads = Some(crate::sce::AdsSpec {
            zoom_fov: nums.first().copied().unwrap_or(50.0).clamp(10.0, 120.0),
            sens_mult: nums.get(1).copied().unwrap_or(0.7).clamp(0.1, 2.0),
            time_in: 0.12,
            blocks_shooting: false,
        });
    }
    if let Some(v) = get_f32(&map, "aoe") {
        w.aoe = v.max(0.0);
    }
    if let Some(v) = get_f32(&map, "viewscale") {
        w.recoil.view_scale = v;
    }
    if let Some(v) = get_f32(&map, "speed") {
        if let WeaponKind::Projectile { speed, .. } = &mut w.kind {
            *speed = v;
        }
    }
    if let Some(v) = get_f32(&map, "grav") {
        if let WeaponKind::Projectile { gravity_scale, .. } = &mut w.kind {
            *gravity_scale = v;
        }
    }
    if let Some(v) = get_f32(&map, "radius") {
        if let WeaponKind::Projectile { radius, .. } = &mut w.kind {
            *radius = v;
        }
    }
    Ok(w)
}

/// 
pub fn parse_player(raw: &str) -> Result<PlayerSpec, String> {
    let (_head, map) = parse_kv(raw);
    let mut p = PlayerSpec::default();
    if let Some(v) = get_vec3(&map, "pos") {
        p.start_pos = v;
    }
    if let Some(v) = get_f32(&map, "yaw") {
        p.start_yaw_deg = v;
    }
    if let Some(v) = get_f32(&map, "hp") {
        p.hp = v;
    }
    if let Some(v) = map.get("preset") {
        p.movement_preset = match v.trim().to_ascii_lowercase().as_str() {
            "static" => MovementPreset::Static,
            "tactical" => MovementPreset::Tactical,
            "arena" => MovementPreset::Arena,
            "br" | "battleroyale" => MovementPreset::BattleRoyale,
            other => return Err(format!("unknown movement preset '{other}'")),
        };
    }
    Ok(p)
}

/// （六个数，中心角点序）。
pub fn parse_block(raw: &str) -> Result<Block, String> {
    let nums: Vec<f32> = raw.split(',').filter_map(|t| t.trim().parse().ok()).collect();
    if nums.len() != 6 {
        return Err(format!("block needs 6 numbers, got '{raw}'"));
    }
    Ok(Block {
        min: Vec3::new(nums[0], nums[1], nums[2]),
        max: Vec3::new(nums[3], nums[4], nums[5]),
    })
}

/// （阈值百分比或绝对分）。
pub fn parse_benchmark(raw: &str) -> Result<Vec<(String, f32)>, String> {
    let mut out = Vec::new();
    for part in raw.split_whitespace() {
        if let Some((k, v)) = part.split_once('=') {
            let f: f32 = v.trim().parse().map_err(|_| format!("bad benchmark '{part}'"))?;
            out.push((k.trim().to_string(), f));
        }
    }
    if out.is_empty() {
        return Err(format!("empty benchmark '{raw}'"));
    }
    Ok(out)
}
