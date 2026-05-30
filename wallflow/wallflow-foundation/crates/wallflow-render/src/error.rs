use thiserror::Error;

/// Errors that can occur during static image rendering.
#[derive(Debug, Error)]
pub enum StaticRenderError {
    #[error("failed to open image: {0}")]
    ImageOpen(String),

    #[error("failed to decode image: {0}")]
    ImageDecode(String),

    #[error("invalid image path: {0}")]
    InvalidImagePath(String),

    #[error("invalid background color: {0}")]
    InvalidBackground(String),

    #[error("invalid viewport dimensions: width={width}, height={height}")]
    InvalidViewport { width: u32, height: u32 },

    #[error("layout error: {0}")]
    Layout(#[from] wallflow_package::LayoutError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
