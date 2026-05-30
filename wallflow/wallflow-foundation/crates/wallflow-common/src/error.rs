use thiserror::Error;

/// Common WallFlow result type.
pub type WallFlowResult<T> = Result<T, WallFlowError>;

/// High-level application errors shared across crates.
#[derive(Debug, Error)]
pub enum WallFlowError {
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("monitor error: {0}")]
    Monitor(String),

    #[error("desktop integration error: {0}")]
    Desktop(String),

    #[error("renderer error: {0}")]
    Renderer(String),

    #[error("media error: {0}")]
    Media(String),

    #[error("ipc error: {0}")]
    Ipc(String),
}
