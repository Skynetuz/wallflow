use thiserror::Error;

/// Errors that can occur during wallpaper package operations.
#[derive(Debug, Error)]
pub enum WallpaperPackageError {
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("unsupported schema version: {0}")]
    UnsupportedSchemaVersion(u32),

    #[error("unsupported wallpaper kind: {0}")]
    UnsupportedKind(String),

    #[error("missing required field: {0}")]
    MissingField(String),

    #[error("asset not found: {0}")]
    AssetNotFound(String),

    #[error("path traversal detected: {0}")]
    PathTraversal(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("validation failed: {errors:?}")]
    Validation { errors: Vec<String> },
}
