#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("RRULE parse error: {0}")]
    RRule(String),

    #[error("Version conflict: expected {expected}, found {actual}")]
    VersionConflict { expected: u64, actual: u64 },
}

pub type Result<T> = std::result::Result<T, Error>;
