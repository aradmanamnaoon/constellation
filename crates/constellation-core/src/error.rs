#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("resource not found: {0}")]
    NotFound(&'static str),
    #[error("resource already exists: {0}")]
    AlreadyExists(&'static str),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("invalid task state transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: crate::TaskState,
        to: crate::TaskState,
    },
    #[error("concurrent task update: expected {expected:?}, found {actual:?}")]
    StateConflict {
        expected: crate::TaskState,
        actual: crate::TaskState,
    },
    #[error("operation ID was reused for different content")]
    OperationConflict,
    #[error("tool action denied: no matching explicit permission")]
    PermissionDenied,
    #[error("store mutex was poisoned")]
    Poisoned,
}

pub type Result<T> = std::result::Result<T, CoreError>;
