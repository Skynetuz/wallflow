//! Presenter abstraction for WallFlow.
//!
//! This module defines the types used by the presenter layer, which takes
//! CPU-rendered RGBA frames and presents them to a window surface (softbuffer)
//! or simulates presentation for cloud testing (presenter-sim).
//!
//! The presenter sits between the CPU reference renderer output and the actual
//! window display. It does not perform any rendering itself — it converts
//! RGBA8 pixel data from the renderer into the format required by the
//! presentation backend.

use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// PresenterBackend
// ---------------------------------------------------------------------------

/// Which backend to use for presenting rendered frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PresenterBackend {
    /// CPU-rendered frames presented via softbuffer (software blit to window).
    /// Works without GPU. Requires a display server for actual windowed mode.
    #[default]
    SoftbufferCpu,
    /// Reserved for future wgpu-based presentation. Not implemented yet.
    WgpuExperimental,
}

impl std::fmt::Display for PresenterBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PresenterBackend::SoftbufferCpu => write!(f, "softbuffer-cpu"),
            PresenterBackend::WgpuExperimental => write!(f, "wgpu-experimental"),
        }
    }
}

impl std::str::FromStr for PresenterBackend {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "softbuffer-cpu" | "softbuffer" | "cpu" => Ok(PresenterBackend::SoftbufferCpu),
            "wgpu-experimental" | "wgpu" => Ok(PresenterBackend::WgpuExperimental),
            other => Err(format!(
                "unknown presenter backend '{}'; expected 'softbuffer-cpu' or 'wgpu-experimental'",
                other
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// PresenterState
// ---------------------------------------------------------------------------

/// Lifecycle state of a presenter instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresenterState {
    /// Presenter has been created but no surface is ready yet.
    Created,
    /// The presentation surface is ready (window created, softbuffer context initialized).
    SurfaceReady,
    /// A frame has been rendered and is ready for presentation.
    FrameRendered,
    /// The frame has been presented to the surface.
    Presented,
    /// The presenter has been resized and needs a re-render.
    Resized,
    /// The presenter has been closed cleanly.
    Closed,
    /// The presenter has encountered a fatal error.
    Failed,
}

impl std::fmt::Display for PresenterState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PresenterState::Created => write!(f, "Created"),
            PresenterState::SurfaceReady => write!(f, "SurfaceReady"),
            PresenterState::FrameRendered => write!(f, "FrameRendered"),
            PresenterState::Presented => write!(f, "Presented"),
            PresenterState::Resized => write!(f, "Resized"),
            PresenterState::Closed => write!(f, "Closed"),
            PresenterState::Failed => write!(f, "Failed"),
        }
    }
}

// ---------------------------------------------------------------------------
// SoftbufferPresenterConfig
// ---------------------------------------------------------------------------

/// Configuration for the softbuffer-based presenter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftbufferPresenterConfig {
    /// Viewport width in pixels.
    pub width: u32,
    /// Viewport height in pixels.
    pub height: u32,
    /// Window title.
    pub title: String,
    /// Source image path (if any). If None, a test pattern is rendered.
    pub source: Option<PathBuf>,
    /// Fit mode for the image within the viewport.
    pub fit: String,
    /// Background color as hex string (e.g. "#000000").
    pub background: String,
    /// Timeout in seconds (0 = no timeout, run until closed).
    pub timeout_secs: u64,
}

impl SoftbufferPresenterConfig {
    /// Validate the configuration. Returns an error message if invalid.
    pub fn validate(&self) -> Result<(), String> {
        if self.width == 0 {
            return Err("viewport width must be > 0".into());
        }
        if self.height == 0 {
            return Err("viewport height must be > 0".into());
        }
        if let Some(ref source) = self.source {
            if !source.exists() {
                return Err(format!("source file does not exist: {}", source.display()));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PresenterReport
// ---------------------------------------------------------------------------

/// Structured report from a presenter run. Used both for real windowed
/// presentation and for cloud-safe simulation mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenterReport {
    /// Which presenter backend was used.
    pub backend: PresenterBackend,
    /// Viewport width and height.
    pub viewport: PresenterViewport,
    /// Source image path (if any).
    pub source_path: Option<String>,
    /// Whether the CPU render completed successfully.
    pub rendered: bool,
    /// Whether the frame was actually presented to a real window surface.
    pub presented: bool,
    /// Whether the presentation was simulated (no real window, cloud-safe).
    pub presented_simulated: bool,
    /// SHA-256 checksum of the RGBA frame data (hex string).
    pub checksum: Option<String>,
    /// Output dimensions (width, height) in pixels.
    pub output_dimensions: Option<PresenterViewport>,
    /// Error message if the presenter failed.
    pub error: Option<String>,
    /// Why the presenter exited (timeout, close-request, error, etc.).
    pub exit_reason: Option<String>,
    /// Duration of the presenter run in milliseconds.
    pub duration_ms: Option<u64>,
}

/// Viewport dimensions for the presenter report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenterViewport {
    pub width: u32,
    pub height: u32,
}

// ---------------------------------------------------------------------------
// Pixel conversion
// ---------------------------------------------------------------------------

/// Convert RGBA8 pixel buffer (from the CPU reference renderer) to the
/// softbuffer `u32` pixel format.
///
/// # Pixel format
///
/// The CPU reference renderer produces RGBA8 pixels: each pixel is 4 bytes
/// in the order R, G, B, A (red first, alpha last).
///
/// softbuffer expects each pixel as a `u32` in the format `0x00RRGGBB`:
/// the highest 8 bits are zero, then red, green, blue in the lowest 24 bits.
/// The alpha channel is **not** represented in the softbuffer format —
/// it is a packed XRGB8888 format (X = ignored, always 0).
///
/// # Alpha handling
///
/// Since softbuffer does not support alpha blending against the desktop,
/// this function composites the RGBA pixels against an opaque black background
/// before discarding the alpha channel. Fully opaque pixels are passed through
/// unchanged; semi-transparent pixels are alpha-blended against black.
///
/// # Endianness
///
/// The `u32` is constructed using native endianness bit operations
/// (`(r as u32) << 16 | (g as u32) << 8 | b as u32`). On little-endian
/// platforms (x86/x86-64), the bytes in memory are B, G, R, 0x00. On
/// big-endian, they are 0x00, R, G, B. This matches what softbuffer expects
/// because it interprets the `u32` as a native-endian integer.
///
/// # Arguments
///
/// * `rgba_pixels` - The RGBA8 pixel buffer from the CPU renderer.
/// * `width` - Width of the frame in pixels.
/// * `height` - Height of the frame in pixels.
///
/// # Returns
///
/// A `Vec<u32>` of length `width * height` in the softbuffer pixel format,
/// or an error string if the buffer length does not match the expected size.
pub fn rgba_to_softbuffer_u32(
    rgba_pixels: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u32>, String> {
    let expected_len = width as usize * height as usize * 4;
    if rgba_pixels.len() != expected_len {
        return Err(format!(
            "RGBA buffer length mismatch: expected {} bytes ({}x{}x4), got {} bytes",
            expected_len,
            width,
            height,
            rgba_pixels.len()
        ));
    }

    let pixel_count = width as usize * height as usize;
    let mut buffer = Vec::with_capacity(pixel_count);

    for chunk in rgba_pixels.chunks_exact(4) {
        let r = chunk[0];
        let g = chunk[1];
        let b = chunk[2];
        let a = chunk[3];

        // Alpha composite against opaque black background
        let (final_r, final_g, final_b) = if a == 255 {
            (r, g, b)
        } else if a == 0 {
            (0, 0, 0)
        } else {
            let af = a as f32 / 255.0;
            (
                (r as f32 * af).round() as u8,
                (g as f32 * af).round() as u8,
                (b as f32 * af).round() as u8,
            )
        };

        // softbuffer format: 0x00RRGGBB
        let pixel = (final_r as u32) << 16 | (final_g as u32) << 8 | (final_b as u32);
        buffer.push(pixel);
    }

    Ok(buffer)
}

/// Convert RGBA8 pixel buffer to softbuffer format for a surface of the given
/// size, handling size mismatches by padding with black or cropping.
///
/// If the rendered frame is smaller than the target surface, the remaining
/// pixels are filled with the background color. If larger, the frame is
/// cropped to the surface size.
///
/// This is used when the rendered image dimensions differ from the window
/// surface dimensions (e.g. after a resize before re-rendering).
pub fn rgba_to_softbuffer_u32_with_surface_size(
    rgba_pixels: &[u8],
    render_width: u32,
    render_height: u32,
    surface_width: NonZeroU32,
    surface_height: NonZeroU32,
    background_color: u32,
) -> Vec<u32> {
    let sw = surface_width.get() as usize;
    let sh = surface_height.get() as usize;
    let rw = render_width as usize;
    let rh = render_height as usize;
    let total = sw * sh;

    let mut buffer = vec![background_color; total];

    // Copy the overlapping region
    let copy_w = rw.min(sw);
    let copy_h = rh.min(sh);

    for y in 0..copy_h {
        for x in 0..copy_w {
            let src_idx = (y * rw + x) * 4;
            let r = rgba_pixels[src_idx];
            let g = rgba_pixels[src_idx + 1];
            let b = rgba_pixels[src_idx + 2];
            let a = rgba_pixels[src_idx + 3];

            let (final_r, final_g, final_b) = if a == 255 {
                (r, g, b)
            } else if a == 0 {
                (0, 0, 0)
            } else {
                let af = a as f32 / 255.0;
                (
                    (r as f32 * af).round() as u8,
                    (g as f32 * af).round() as u8,
                    (b as f32 * af).round() as u8,
                )
            };

            let pixel = (final_r as u32) << 16 | (final_g as u32) << 8 | (final_b as u32);
            let dst_idx = y * sw + x;
            buffer[dst_idx] = pixel;
        }
    }

    buffer
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a 1x1 RGBA pixel buffer.
    fn rgba_pixel(r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
        vec![r, g, b, a]
    }

    /// Helper: create an NxM RGBA pixel buffer filled with the same pixel.
    fn rgba_filled(w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
        let mut buf = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            buf.push(r);
            buf.push(g);
            buf.push(b);
            buf.push(a);
        }
        buf
    }

    #[test]
    fn test_rgba_to_softbuffer_red() {
        let pixels = rgba_pixel(255, 0, 0, 255);
        let result = rgba_to_softbuffer_u32(&pixels, 1, 1).unwrap();
        assert_eq!(result[0], 0x00FF0000, "red pixel should be 0x00FF0000");
    }

    #[test]
    fn test_rgba_to_softbuffer_green() {
        let pixels = rgba_pixel(0, 255, 0, 255);
        let result = rgba_to_softbuffer_u32(&pixels, 1, 1).unwrap();
        assert_eq!(result[0], 0x0000FF00, "green pixel should be 0x0000FF00");
    }

    #[test]
    fn test_rgba_to_softbuffer_blue() {
        let pixels = rgba_pixel(0, 0, 255, 255);
        let result = rgba_to_softbuffer_u32(&pixels, 1, 1).unwrap();
        assert_eq!(result[0], 0x000000FF, "blue pixel should be 0x000000FF");
    }

    #[test]
    fn test_rgba_to_softbuffer_white() {
        let pixels = rgba_pixel(255, 255, 255, 255);
        let result = rgba_to_softbuffer_u32(&pixels, 1, 1).unwrap();
        assert_eq!(result[0], 0x00FFFFFF, "white pixel should be 0x00FFFFFF");
    }

    #[test]
    fn test_rgba_to_softbuffer_black() {
        let pixels = rgba_pixel(0, 0, 0, 255);
        let result = rgba_to_softbuffer_u32(&pixels, 1, 1).unwrap();
        assert_eq!(result[0], 0x00000000, "black pixel should be 0x00000000");
    }

    #[test]
    fn test_rgba_to_softbuffer_transparent_is_black() {
        // Fully transparent pixels are composited against black → black
        let pixels = rgba_pixel(128, 64, 32, 0);
        let result = rgba_to_softbuffer_u32(&pixels, 1, 1).unwrap();
        assert_eq!(
            result[0], 0x00000000,
            "transparent pixel should composite to black"
        );
    }

    #[test]
    fn test_rgba_to_softbuffer_semi_transparent() {
        // Semi-transparent pixel: alpha = 128/255 ≈ 0.502
        // R=200 * 0.502 ≈ 100, G=100 * 0.502 ≈ 50, B=50 * 0.502 ≈ 25
        let pixels = rgba_pixel(200, 100, 50, 128);
        let result = rgba_to_softbuffer_u32(&pixels, 1, 1).unwrap();
        let expected_r = (200.0f32 * 128.0f32 / 255.0f32).round() as u32;
        let expected_g = (100.0f32 * 128.0f32 / 255.0f32).round() as u32;
        let expected_b = (50.0f32 * 128.0f32 / 255.0f32).round() as u32;
        let expected = expected_r << 16 | expected_g << 8 | expected_b;
        assert_eq!(
            result[0], expected,
            "semi-transparent pixel should be alpha-composited against black"
        );
    }

    #[test]
    fn test_rgba_to_softbuffer_invalid_length() {
        let pixels = vec![255, 0, 0]; // Only 3 bytes, not 4
        let result = rgba_to_softbuffer_u32(&pixels, 1, 1);
        assert!(result.is_err(), "should fail with invalid buffer length");
        let err = result.unwrap_err();
        assert!(
            err.contains("mismatch"),
            "error message should mention mismatch: {err}"
        );
    }

    #[test]
    fn test_rgba_to_softbuffer_wrong_dimensions() {
        // 2x2 frame = 16 bytes, but we pass 1x1 = 4 bytes expected
        let pixels = rgba_filled(2, 2, 255, 0, 0, 255); // 16 bytes
        let result = rgba_to_softbuffer_u32(&pixels, 1, 1); // expects 4 bytes
        assert!(result.is_err(), "should fail with wrong dimensions");
    }

    #[test]
    fn test_rgba_to_softbuffer_multi_pixel() {
        // 2x1 frame: red and blue
        let pixels = vec![255, 0, 0, 255, 0, 0, 255, 255];
        let result = rgba_to_softbuffer_u32(&pixels, 2, 1).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], 0x00FF0000, "first pixel should be red");
        assert_eq!(result[1], 0x000000FF, "second pixel should be blue");
    }

    // --- PresenterBackend tests ---

    #[test]
    fn test_presenter_backend_display() {
        assert_eq!(
            format!("{}", PresenterBackend::SoftbufferCpu),
            "softbuffer-cpu"
        );
        assert_eq!(
            format!("{}", PresenterBackend::WgpuExperimental),
            "wgpu-experimental"
        );
    }

    #[test]
    fn test_presenter_backend_from_str() {
        assert_eq!(
            "softbuffer-cpu".parse::<PresenterBackend>(),
            Ok(PresenterBackend::SoftbufferCpu)
        );
        assert_eq!(
            "softbuffer".parse::<PresenterBackend>(),
            Ok(PresenterBackend::SoftbufferCpu)
        );
        assert_eq!(
            "cpu".parse::<PresenterBackend>(),
            Ok(PresenterBackend::SoftbufferCpu)
        );
        assert_eq!(
            "wgpu".parse::<PresenterBackend>(),
            Ok(PresenterBackend::WgpuExperimental)
        );
        assert_eq!(
            "wgpu-experimental".parse::<PresenterBackend>(),
            Ok(PresenterBackend::WgpuExperimental)
        );
        assert!("unknown".parse::<PresenterBackend>().is_err());
    }

    #[test]
    fn test_presenter_backend_serde_roundtrip() {
        let backend = PresenterBackend::SoftbufferCpu;
        let json = serde_json::to_string(&backend).unwrap();
        let parsed: PresenterBackend = serde_json::from_str(&json).unwrap();
        assert_eq!(backend, parsed);
    }

    // --- PresenterState tests ---

    #[test]
    fn test_presenter_state_display() {
        assert_eq!(format!("{}", PresenterState::Created), "Created");
        assert_eq!(format!("{}", PresenterState::SurfaceReady), "SurfaceReady");
        assert_eq!(format!("{}", PresenterState::Failed), "Failed");
    }

    #[test]
    fn test_presenter_state_serde_roundtrip() {
        for state in [
            PresenterState::Created,
            PresenterState::SurfaceReady,
            PresenterState::FrameRendered,
            PresenterState::Presented,
            PresenterState::Resized,
            PresenterState::Closed,
            PresenterState::Failed,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let parsed: PresenterState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, parsed);
        }
    }

    // --- SoftbufferPresenterConfig tests ---

    #[test]
    fn test_config_valid() {
        let config = SoftbufferPresenterConfig {
            width: 800,
            height: 450,
            title: "Test".into(),
            source: None,
            fit: "cover".into(),
            background: "#000000".into(),
            timeout_secs: 5,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_zero_width_rejected() {
        let config = SoftbufferPresenterConfig {
            width: 0,
            height: 450,
            title: "Test".into(),
            source: None,
            fit: "cover".into(),
            background: "#000000".into(),
            timeout_secs: 5,
        };
        let err = config.validate().unwrap_err();
        assert!(err.contains("width"), "error should mention width: {err}");
    }

    #[test]
    fn test_config_zero_height_rejected() {
        let config = SoftbufferPresenterConfig {
            width: 800,
            height: 0,
            title: "Test".into(),
            source: None,
            fit: "cover".into(),
            background: "#000000".into(),
            timeout_secs: 5,
        };
        let err = config.validate().unwrap_err();
        assert!(err.contains("height"), "error should mention height: {err}");
    }

    #[test]
    fn test_config_invalid_source_handled() {
        let config = SoftbufferPresenterConfig {
            width: 800,
            height: 450,
            title: "Test".into(),
            source: Some(PathBuf::from("/nonexistent/path.png")),
            fit: "cover".into(),
            background: "#000000".into(),
            timeout_secs: 5,
        };
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("does not exist") || err.contains("nonexistent"),
            "error should mention missing file: {err}"
        );
    }

    // --- PresenterReport tests ---

    #[test]
    fn test_presenter_report_serde_roundtrip() {
        let report = PresenterReport {
            backend: PresenterBackend::SoftbufferCpu,
            viewport: PresenterViewport {
                width: 800,
                height: 450,
            },
            source_path: Some("/test/image.png".into()),
            rendered: true,
            presented: false,
            presented_simulated: true,
            checksum: Some("abc123".into()),
            output_dimensions: Some(PresenterViewport {
                width: 800,
                height: 450,
            }),
            error: None,
            exit_reason: Some("timeout".into()),
            duration_ms: Some(150),
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: PresenterReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report.backend, parsed.backend);
        assert_eq!(report.rendered, parsed.rendered);
        assert_eq!(report.presented_simulated, parsed.presented_simulated);
        assert_eq!(report.checksum, parsed.checksum);
    }

    #[test]
    fn test_presenter_report_error_case_serde() {
        let report = PresenterReport {
            backend: PresenterBackend::SoftbufferCpu,
            viewport: PresenterViewport {
                width: 0,
                height: 0,
            },
            source_path: None,
            rendered: false,
            presented: false,
            presented_simulated: false,
            checksum: None,
            output_dimensions: None,
            error: Some("no display server".into()),
            exit_reason: Some("error".into()),
            duration_ms: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: PresenterReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report.error, parsed.error);
    }

    // --- rgba_to_softbuffer_u32_with_surface_size tests ---

    #[test]
    fn test_rgba_to_softbuffer_padded_with_bg() {
        // Render a 1x1 red pixel for a 2x2 surface — should pad with background
        let pixels = rgba_pixel(255, 0, 0, 255);
        let bg = 0x00808080; // grey background
        let result = rgba_to_softbuffer_u32_with_surface_size(
            &pixels,
            1,
            1,
            NonZeroU32::new(2).unwrap(),
            NonZeroU32::new(2).unwrap(),
            bg,
        );
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], 0x00FF0000, "top-left should be red");
        assert_eq!(result[1], bg, "top-right should be background");
        assert_eq!(result[2], bg, "bottom-left should be background");
        assert_eq!(result[3], bg, "bottom-right should be background");
    }

    #[test]
    fn test_rgba_to_softbuffer_cropped() {
        // Render 2x2 but present to 1x1 — should crop
        let pixels = rgba_filled(2, 2, 255, 0, 0, 255); // red 2x2
        let bg = 0x00000000;
        let result = rgba_to_softbuffer_u32_with_surface_size(
            &pixels,
            2,
            2,
            NonZeroU32::new(1).unwrap(),
            NonZeroU32::new(1).unwrap(),
            bg,
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], 0x00FF0000, "should be red (cropped from 2x2)");
    }
}
