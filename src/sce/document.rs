//! `.sce` 的分段文档解析。
//!
//! KovaaK 格式 = INI 头 + 若干 `[Xxx Profile]` 段 + 可选的
//! `[Shot Extension]`（本软件扩展段）+ 可选的尾部 JSON（地图数据）。
//!
//! [`SceDocument::parse`] 只做「把键值收进各段」这一件事，不解释语义；
//! 语义在 [`crate::sce::convert`] 里。

use std::collections::HashMap;

/// 一段 `[Xxx]` 下的键值对。`name` 取段内 `Name=`（没有就用段头名）。
#[derive(Clone, Debug, Default)]
pub struct KvSection {
    pub name: String,
    pub kv: HashMap<String, String>,
}

impl KvSection {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.kv.get(key).map(String::as_str)
    }

    /// 浮点；解析失败 / 非有限值返回 None。
    pub fn f32(&self, key: &str) -> Option<f32> {
        self.get(key)
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite())
    }

    /// `(X=… Y=… Z=…)` 三元组（KovaaK 的向量写法，空格分隔）。
    pub fn xyz(&self, key: &str) -> (f32, f32, f32) {
        parse_xyz(self.get(key).unwrap_or(""))
    }

    /// 分号分隔列表（`AddedBots` 这种）。
    pub fn list(&self, key: &str) -> Vec<String> {
        split_list(self.kv.get(key))
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.get(key)?.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        }
    }
}

/// 一份 `.sce` 切出来的全部内容。
#[derive(Clone, Debug, Default)]
pub struct SceDocument {
    /// 首个 `[` 之前的全局键值（Name/Timelimit/AddedBots/MapName…）。
    pub header: HashMap<String, String>,
    /// `[Aim Profile]` 段。
    pub aim: Vec<KvSection>,
    /// `[Bot Profile]` 段。
    pub bots: Vec<KvSection>,
    /// `[Character Profile]` 段。
    pub characters: Vec<KvSection>,
    /// `[Dodge Profile]` 段。
    pub dodges: Vec<KvSection>,
    /// `[Weapon Profile]` 段。
    pub weapons: Vec<KvSection>,
    /// `[Shot Extension]` 段（本软件扩展）。
    pub shot_ext: Option<KvSection>,
    /// 段头不认识的其它段，照样收着，不丢数据。
    pub other: Vec<(String, KvSection)>,
    /// 文件尾部的地图 JSON（原样保留，M3 才解释）。
    pub map_data: Option<serde_json::Value>,
    /// 解析期间攒下的非致命问题。
    pub warnings: Vec<SceWarning>,
}

/// 非致命解析问题（键不认识、数值非法…）。`load` 不因此失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceWarning {
    pub message: String,
}

impl SceWarning {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl SceDocument {
    /// 按 `[Xxx]` 段切分。header = 首个 `\n[` 之前的部分；
    /// 遇到以 `{` 开头的行停止收段，余下整段当 JSON 试解析。
    pub fn parse(raw: &str) -> Self {
        let mut doc = SceDocument {
            header: parse_kv(&raw[..raw.find("\n[").unwrap_or(raw.len())]),
            ..Default::default()
        };
        let mut header_name = String::new();
        let mut kv: Option<HashMap<String, String>> = None;
        // 逐行扫；段头名（`[` `]` 之间）留到见到下一段时统一归桶。
        let mut sections: Vec<(String, HashMap<String, String>)> = Vec::new();
        let mut lines = raw.lines();
        let mut byte_offset = 0usize;
        while let Some(line) = lines.next() {
            let line_start = byte_offset;
            byte_offset += line.len() + 1;
            let trimmed = line.trim();
            if trimmed.starts_with('{') {
                // JSON 地图数据从这里到文件尾。
                let rest = &raw[line_start..];
                match serde_json::from_str::<serde_json::Value>(rest) {
                    Ok(value) => doc.map_data = Some(value),
                    Err(err) => doc
                        .warnings
                        .push(SceWarning::new(format!("地图 JSON 解析失败: {err}"))),
                }
                break;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if let (name, Some(map)) = (std::mem::take(&mut header_name), kv.take()) {
                    sections.push((name, map));
                }
                header_name = trimmed[1..trimmed.len() - 1].trim().to_string();
                kv = Some(HashMap::new());
                continue;
            }
            if let Some(map) = kv.as_mut() {
                if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = trimmed.split_once('=') {
                    map.insert(key.trim().to_string(), value.trim().to_string());
                }
            }
        }
        if let (name, Some(map)) = (header_name, kv) {
            sections.push((name, map));
        }
        for (header, map) in sections {
            let name = map
                .get("Name")
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| header.clone());
            let section = KvSection { name, kv: map };
            match header.as_str() {
                "Aim Profile" => doc.aim.push(section),
                "Bot Profile" => doc.bots.push(section),
                "Character Profile" => doc.characters.push(section),
                "Dodge Profile" => doc.dodges.push(section),
                "Weapon Profile" => doc.weapons.push(section),
                "Shot Extension" => doc.shot_ext = Some(section),
                other => doc.other.push((other.to_string(), section)),
            }
        }
        doc
    }

    /// 所有可能当「bot profile」用的段（老 `parse_profiles` 的等价物：
    /// 每个带 `Name` 键的段，无论段头叫什么；`[Shot Extension]` 除外）。
    pub fn profiles(&self) -> Vec<&KvSection> {
        self.bots
            .iter()
            .chain(self.aim.iter())
            .chain(self.characters.iter())
            .chain(self.dodges.iter())
            .chain(self.weapons.iter())
            .chain(self.other.iter().map(|(_, s)| s))
            .filter(|section| section.kv.contains_key("Name"))
            .collect()
    }
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
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    map
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
