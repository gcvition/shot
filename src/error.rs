use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ShotError {
    #[error("invalid sensitivity: {0}")]
    Sensitivity(String),
    #[error("invalid resolution {0}x{1}")]
    Resolution(u32, u32),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("trace encode error: {0}")]
    Trace(#[from] postcard::Error),
    #[error("missing session {0}")]
    MissingSession(String),
    #[error("missing file {0}")]
    MissingFile(PathBuf),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ShotError>;
