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

/// Metadata extracted from a decoded image file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub color_type: String,
    pub detected_format: String,
    pub file_size_bytes: u64,
}

/// Error during image decoding.
#[derive(Debug, Error)]
pub enum ImageDecodeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image decode error: {0}")]
    Decode(String),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
}

/// Load image metadata from a file without fully decoding pixels.
///
/// Uses `image::ImageReader` to read dimensions and format information.
pub fn load_image_metadata(path: &std::path::Path) -> Result<ImageMetadata, ImageDecodeError> {
    let file_size = std::fs::metadata(path)?.len();

    let reader = image::ImageReader::open(path).map_err(|e| {
        ImageDecodeError::Io(std::io::Error::new(
            e.kind(),
            format!("failed to open image for metadata: {e}"),
        ))
    })?;

    let detected_format = reader
        .format()
        .map(|f| format!("{f:?}"))
        .unwrap_or_else(|| "Unknown".to_string());

    let (width, height) = reader
        .into_dimensions()
        .map_err(|e| ImageDecodeError::Decode(format!("failed to read image dimensions: {e}")))?;

    // Re-open to get color type without full decode
    let reader2 = image::ImageReader::open(path).map_err(|e| {
        ImageDecodeError::Io(std::io::Error::new(
            e.kind(),
            format!("failed to open image for color type: {e}"),
        ))
    })?;

    let color_type_str = reader2
        .decode()
        .map(|img| format!("{:?}", img.color()))
        .unwrap_or_else(|_| "Unknown".to_string());

    Ok(ImageMetadata {
        width,
        height,
        color_type: color_type_str,
        detected_format,
        file_size_bytes: file_size,
    })
}
