use thiserror::Error;
use vantage_core::VantageError;

#[derive(Debug, Error)]
pub enum LensBuildError {
    #[error("cache backend is required (call .cache_at(...) or .cache_source(...))")]
    MissingCacheSource,

    #[error("augmentations were registered but no catalog was provided (call .catalog(...))")]
    MissingCatalog,

    #[error(transparent)]
    Other(#[from] VantageError),
}

#[derive(Debug, Error)]
pub enum DioError {
    #[error("write queue is full")]
    WriteQueueFull,

    #[error("dio is shutting down")]
    ShuttingDown,

    #[error(transparent)]
    Other(#[from] VantageError),
}
