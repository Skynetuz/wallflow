use thiserror::Error;

/// Errors that can occur during wgpu GPU rendering.
#[derive(Debug, Error)]
pub enum WgpuRenderError {
    #[error("no suitable GPU adapter available: {0}")]
    NoAdapter(String),

    #[error("failed to create GPU device: {0}")]
    DeviceCreation(String),

    #[error("GPU render error: {0}")]
    RenderFailed(String),

    #[error("GPU buffer mapping failed: {0}")]
    BufferMap(String),

    #[error("GPU feature not supported: {0}")]
    FeatureNotSupported(String),

    #[error("invalid viewport for GPU render: width={width}, height={height}")]
    InvalidViewport { width: u32, height: u32 },

    #[error("invalid background color for GPU render: {0}")]
    InvalidBackground(String),

    #[error("wgpu backend is experimental and this operation is not yet implemented: {0}")]
    NotImplemented(String),
}
