//! `.shot` 的字节编解码。
//!
//! 磁盘上可能出现三种格式（按 [`decode`] 的分派顺序）：
//! 1. `SHOT2` 魔数开头 → zstd(postcard(v2))，当前格式
//! 2. `{` 开头 → v1 JSON（旧调试格式，纯文本）
//! 3. 其他 → v1 裸 postcard
//!
//! 只往里读不写回：旧格式**不会**被自动转存成新格式，避免悄悄改写存档。

use super::v1::SessionFileV1;
use super::v2::{MAGIC_V2, SessionFile};
use crate::error::Result;

/// zstd 压缩等级。3 是体积/速度的平衡点（局结束写盘预算 300ms 内）。
const ZSTD_LEVEL: i32 = 3;

/// 读出来的轨迹，还没统一版本。回放和分析都只面对 v2。
pub enum SessionAny {
    V1(SessionFileV1),
    V2(SessionFile),
}

impl SessionAny {
    pub fn into_v2(self) -> SessionFile {
        match self {
            Self::V1(file) => file.into_v2(),
            Self::V2(file) => file,
        }
    }
}

/// v2 → 磁盘字节。
pub fn encode_v2(file: &SessionFile) -> Result<Vec<u8>> {
    let packed = postcard::to_allocvec(file)?;
    let compressed = zstd::stream::encode_all(&packed[..], ZSTD_LEVEL)?;
    let mut out = Vec::with_capacity(MAGIC_V2.len() + compressed.len());
    out.extend_from_slice(MAGIC_V2);
    out.extend_from_slice(&compressed);
    Ok(out)
}

/// 字节 → [`SessionAny`]。见模块文档的分派顺序。
pub fn decode(bytes: &[u8]) -> Result<SessionAny> {
    if bytes.starts_with(MAGIC_V2) {
        // 解压上限 64MB：防 zip bomb。
        let mut raw = &bytes[MAGIC_V2.len()..];
        let packed = {
            let mut out = Vec::new();
            zstd::stream::copy_decode(&mut raw, &mut out)?;
            if out.len() > 64 * 1024 * 1024 {
                return Err(crate::error::ShotError::Other(
                    "session trace too large after decode".into(),
                ));
            }
            out
        };
        return Ok(SessionAny::V2(postcard::from_bytes(&packed)?));
    }
    if looks_like_json(bytes) {
        let file: SessionFileV1 = serde_json::from_slice(bytes)?;
        return Ok(SessionAny::V1(file));
    }
    Ok(SessionAny::V1(postcard::from_bytes(bytes)?))
}

fn looks_like_json(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .find(|b| !b.is_ascii_whitespace())
        .is_some_and(|b| *b == b'{')
}
