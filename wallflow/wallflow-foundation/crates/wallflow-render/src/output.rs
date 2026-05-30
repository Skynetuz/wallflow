use serde::{Deserialize, Serialize};

/// The output of a static image render operation.
///
/// Contains the RGBA pixel buffer and optional metadata about the output.
#[derive(Debug, Clone)]
pub struct RenderOutput {
    /// Width of the output in pixels.
    pub width: u32,
    /// Height of the output in pixels.
    pub height: u32,
    /// Raw RGBA pixel data (4 bytes per pixel, row-major, top-to-bottom).
    pub pixels_rgba: Vec<u8>,
    /// Optional path where the output was saved as PNG.
    pub output_path: Option<std::path::PathBuf>,
}

impl RenderOutput {
    /// Create a new render output with the given dimensions and pixel data.
    pub fn new(width: u32, height: u32, pixels_rgba: Vec<u8>) -> Self {
        Self {
            width,
            height,
            pixels_rgba,
            output_path: None,
        }
    }

    /// Save the render output as a PNG file.
    pub fn save_png(&mut self, path: &std::path::Path) -> Result<(), crate::StaticRenderError> {
        let img = image::RgbaImage::from_raw(self.width, self.height, self.pixels_rgba.clone())
            .ok_or_else(|| {
                crate::StaticRenderError::ImageDecode(format!(
                    "failed to create RGBA image from buffer ({}x{}, {} bytes)",
                    self.width,
                    self.height,
                    self.pixels_rgba.len()
                ))
            })?;

        img.save(path).map_err(|e| {
            crate::StaticRenderError::Io(std::io::Error::other(format!(
                "failed to save PNG to {}: {}",
                path.display(),
                e
            )))
        })?;

        self.output_path = Some(path.to_path_buf());
        Ok(())
    }

    /// Compute SHA-256 checksum of the RGBA pixel data.
    pub fn checksum_sha256(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&self.pixels_rgba);
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    /// Compute metadata for this render output.
    pub fn metadata(&self) -> RenderOutputMetadata {
        RenderOutputMetadata {
            width: self.width,
            height: self.height,
            pixel_format: "RGBA8".to_string(),
            file_size_bytes: self
                .output_path
                .as_ref()
                .and_then(|p| std::fs::metadata(p).ok().map(|m| m.len())),
            checksum: Some(self.checksum_sha256()),
        }
    }
}

/// Metadata about a render output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderOutputMetadata {
    /// Width of the output in pixels.
    pub width: u32,
    /// Height of the output in pixels.
    pub height: u32,
    /// Pixel format (e.g., "RGBA8").
    pub pixel_format: String,
    /// File size in bytes (only set if output was saved to disk).
    pub file_size_bytes: Option<u64>,
    /// SHA-256 checksum of the RGBA pixel data.
    pub checksum: Option<String>,
}
