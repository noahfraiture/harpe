use thiserror::Error;

pub type Result<T> = std::result::Result<T, HarpeError>;

#[derive(Debug, Error)]
pub enum HarpeError {
    #[error("validation error: {0}")]
    Validation(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("store error: {0}")]
    Store(String),

    #[error("llm error: {0}")]
    Llm(String),
}

impl From<surrealdb::Error> for HarpeError {
    fn from(value: surrealdb::Error) -> Self {
        Self::Store(value.to_string())
    }
}
