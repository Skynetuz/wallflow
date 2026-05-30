use crate::color::RgbaColor;
use crate::error::StaticRenderError;
use crate::output::RenderOutput;
use wallflow_common::FitMode;

/// Input for a static image render operation.
#[derive(Debug, Clone)]
pub struct StaticRenderInput {
    /// Path to the source image file.
    pub image_path: std::path::PathBuf,
    /// Viewport dimensions.
    pub viewport: wallflow_package::Viewport,
    /// Fit mode for the image.
    pub fit: FitMode,
    /// Background color as a hex string (e.g., "#000000").
    pub background: String,
    /// Optional opacity (0–255). Defaults to 255 (fully opaque) if None.
    pub opacity: Option<u8>,
}

/// Render a static image to an RGBA buffer using the CPU reference renderer.
///
/// This function:
/// 1. Opens and decodes the image using the `image` crate.
/// 2. Converts the image to RGBA8.
/// 3. Creates an output buffer the size of the viewport.
/// 4. Fills the buffer with the background color.
/// 5. Calculates the layout using the existing layout engine.
/// 6. Draws the image into the output buffer according to the fit mode.
/// 7. Returns the `RenderOutput`.
///
/// Uses nearest-neighbor scaling (temporary; bilinear/Lanczos can be added later).
pub fn render_static_image_cpu(
    input: StaticRenderInput,
) -> Result<RenderOutput, StaticRenderError> {
    // Validate viewport
    if input.viewport.width == 0 || input.viewport.height == 0 {
        return Err(StaticRenderError::InvalidViewport {
            width: input.viewport.width,
            height: input.viewport.height,
        });
    }

    // Parse background color
    let bg_color =
        RgbaColor::parse_hex(&input.background).map_err(StaticRenderError::InvalidBackground)?;

    // Open and decode image
    let img = image::open(&input.image_path).map_err(|e| {
        StaticRenderError::ImageOpen(format!("{}: {}", input.image_path.display(), e))
    })?;
    let img_rgba = img.to_rgba8();
    let img_width = img_rgba.width();
    let img_height = img_rgba.height();

    if img_width == 0 || img_height == 0 {
        return Err(StaticRenderError::ImageDecode(format!(
            "decoded image has zero dimensions: {}x{}",
            img_width, img_height
        )));
    }

    // Calculate layout
    let image_size = wallflow_package::ImageSize {
        width: img_width,
        height: img_height,
    };
    let layout = wallflow_package::calculate_static_image_layout(
        image_size,
        input.viewport,
        input.fit,
        input.background.clone(),
    )?;

    let vp_width = input.viewport.width as usize;
    let vp_height = input.viewport.height as usize;

    // Create output buffer filled with background color
    let bg_pixel = [bg_color.r, bg_color.g, bg_color.b, bg_color.a];
    let mut output = vec![0u8; vp_width * vp_height * 4];
    for pixel in output.chunks_exact_mut(4) {
        pixel.copy_from_slice(&bg_pixel);
    }

    // Apply opacity to the image pixels if needed
    let opacity = input.opacity.unwrap_or(255);
    let source_pixels: Vec<u8> = if opacity < 255 {
        img_rgba
            .pixels()
            .flat_map(|p| {
                let r = ((p.0[0] as u32) * (opacity as u32) + 127) / 255;
                let g = ((p.0[1] as u32) * (opacity as u32) + 127) / 255;
                let b = ((p.0[2] as u32) * (opacity as u32) + 127) / 255;
                let a = ((p.0[3] as u32) * (opacity as u32) + 127) / 255;
                [r as u8, g as u8, b as u8, a as u8]
            })
            .collect()
    } else {
        img_rgba.into_raw()
    };

    // Draw the image according to the fit mode
    match input.fit {
        FitMode::Cover | FitMode::Contain | FitMode::Stretch | FitMode::Center => {
            draw_scaled_or_centered(
                &mut output,
                vp_width,
                vp_height,
                &source_pixels,
                img_width,
                img_height,
                &layout.destination_rect,
            );
        }
        FitMode::Tile => {
            draw_tiled(
                &mut output,
                vp_width,
                vp_height,
                &source_pixels,
                img_width,
                img_height,
            );
        }
    }

    Ok(RenderOutput::new(
        input.viewport.width,
        input.viewport.height,
        output,
    ))
}

/// Draw an image scaled/positioned according to the destination rectangle.
///
/// This handles Cover, Contain, Stretch, and Center modes. For Cover,
/// parts of the image that extend beyond the viewport are clipped.
fn draw_scaled_or_centered(
    output: &mut [u8],
    vp_width: usize,
    vp_height: usize,
    source_pixels: &[u8],
    img_width: u32,
    img_height: u32,
    dest_rect: &wallflow_package::RenderRect,
) {
    let dx_start = dest_rect.x.max(0.0) as usize;
    let dy_start = dest_rect.y.max(0.0) as usize;
    let dx_end = ((dest_rect.x + dest_rect.width) as usize).min(vp_width);
    let dy_end = ((dest_rect.y + dest_rect.height) as usize).min(vp_height);

    let iw = img_width as f64;
    let ih = img_height as f64;
    let dw = dest_rect.width;
    let dh = dest_rect.height;

    for dy in dy_start..dy_end {
        for dx in dx_start..dx_end {
            // Map destination pixel to source pixel (nearest-neighbor)
            let sx_f = ((dx as f64) - dest_rect.x) / dw * iw;
            let sy_f = ((dy as f64) - dest_rect.y) / dh * ih;

            let sx = (sx_f as usize).min(img_width as usize - 1);
            let sy = (sy_f as usize).min(img_height as usize - 1);

            let src_idx = (sy * (img_width as usize) + sx) * 4;
            let dst_idx = (dy * vp_width + dx) * 4;

            if src_idx + 4 <= source_pixels.len() && dst_idx + 4 <= output.len() {
                let sa = source_pixels[src_idx + 3] as u32;
                if sa == 255 {
                    output[dst_idx..dst_idx + 4]
                        .copy_from_slice(&source_pixels[src_idx..src_idx + 4]);
                } else if sa > 0 {
                    // Alpha blend
                    let sr = source_pixels[src_idx] as u32;
                    let sg = source_pixels[src_idx + 1] as u32;
                    let sb = source_pixels[src_idx + 2] as u32;
                    let dr = output[dst_idx] as u32;
                    let dg = output[dst_idx + 1] as u32;
                    let db = output[dst_idx + 2] as u32;
                    let da = output[dst_idx + 3] as u32;

                    let alpha = sa;
                    let inv_alpha = 255 - alpha;

                    output[dst_idx] = ((sr * alpha + dr * inv_alpha + 127) / 255) as u8;
                    output[dst_idx + 1] = ((sg * alpha + dg * inv_alpha + 127) / 255) as u8;
                    output[dst_idx + 2] = ((sb * alpha + db * inv_alpha + 127) / 255) as u8;
                    output[dst_idx + 3] = ((sa * 255 + da * inv_alpha + 127) / 255) as u8;
                }
                // If sa == 0, keep the background pixel
            }
        }
    }
}

/// Draw an image tiled across the viewport.
fn draw_tiled(
    output: &mut [u8],
    vp_width: usize,
    vp_height: usize,
    source_pixels: &[u8],
    img_width: u32,
    img_height: u32,
) {
    let iw = img_width as usize;
    let ih = img_height as usize;

    for y in 0..vp_height {
        let sy = y % ih;
        for x in 0..vp_width {
            let sx = x % iw;

            let src_idx = (sy * iw + sx) * 4;
            let dst_idx = (y * vp_width + x) * 4;

            if src_idx + 4 <= source_pixels.len() && dst_idx + 4 <= output.len() {
                let sa = source_pixels[src_idx + 3] as u32;
                if sa == 255 {
                    output[dst_idx..dst_idx + 4]
                        .copy_from_slice(&source_pixels[src_idx..src_idx + 4]);
                } else if sa > 0 {
                    let sr = source_pixels[src_idx] as u32;
                    let sg = source_pixels[src_idx + 1] as u32;
                    let sb = source_pixels[src_idx + 2] as u32;
                    let dr = output[dst_idx] as u32;
                    let dg = output[dst_idx + 1] as u32;
                    let db = output[dst_idx + 2] as u32;
                    let da = output[dst_idx + 3] as u32;

                    let alpha = sa;
                    let inv_alpha = 255 - alpha;

                    output[dst_idx] = ((sr * alpha + dr * inv_alpha + 127) / 255) as u8;
                    output[dst_idx + 1] = ((sg * alpha + dg * inv_alpha + 127) / 255) as u8;
                    output[dst_idx + 2] = ((sb * alpha + db * inv_alpha + 127) / 255) as u8;
                    output[dst_idx + 3] = ((sa * 255 + da * inv_alpha + 127) / 255) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a test PNG at the given path with the given pixel colors.
    fn create_test_png(
        path: &std::path::Path,
        width: u32,
        height: u32,
        pixels: &[image::Rgba<u8>],
    ) {
        let mut img = image::RgbaImage::new(width, height);
        for (i, pixel) in pixels.iter().enumerate() {
            let x = i as u32 % width;
            let y = i as u32 / width;
            if x < width && y < height {
                img.put_pixel(x, y, *pixel);
            }
        }
        img.save(path).expect("save test PNG");
    }

    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_test_dir() -> std::path::PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("wallflow-render-tests-{id}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    // --- Dimension tests ---

    #[test]
    fn render_cover_creates_correct_output_dimensions() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        create_test_png(
            &img_path,
            2,
            2,
            &[
                image::Rgba([255, 0, 0, 255]),
                image::Rgba([0, 255, 0, 255]),
                image::Rgba([0, 0, 255, 255]),
                image::Rgba([255, 255, 255, 255]),
            ],
        );

        let input = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 800,
                height: 450,
            },
            fit: FitMode::Cover,
            background: "#000000".into(),
            opacity: None,
        };

        let output = render_static_image_cpu(input).expect("render");
        assert_eq!(output.width, 800);
        assert_eq!(output.height, 450);
        assert_eq!(output.pixels_rgba.len(), 800 * 450 * 4);
    }

    #[test]
    fn render_contain_creates_correct_output_dimensions() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        create_test_png(&img_path, 2, 2, &[image::Rgba([0, 0, 0, 255]); 4]);

        let input = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 800,
                height: 450,
            },
            fit: FitMode::Contain,
            background: "#000000".into(),
            opacity: None,
        };

        let output = render_static_image_cpu(input).expect("render");
        assert_eq!(output.width, 800);
        assert_eq!(output.height, 450);
    }

    #[test]
    fn render_stretch_creates_correct_output_dimensions() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        create_test_png(&img_path, 2, 2, &[image::Rgba([0, 0, 0, 255]); 4]);

        let input = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 800,
                height: 450,
            },
            fit: FitMode::Stretch,
            background: "#000000".into(),
            opacity: None,
        };

        let output = render_static_image_cpu(input).expect("render");
        assert_eq!(output.width, 800);
        assert_eq!(output.height, 450);
    }

    #[test]
    fn render_center_creates_correct_output_dimensions() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        create_test_png(&img_path, 2, 2, &[image::Rgba([0, 0, 0, 255]); 4]);

        let input = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 800,
                height: 450,
            },
            fit: FitMode::Center,
            background: "#000000".into(),
            opacity: None,
        };

        let output = render_static_image_cpu(input).expect("render");
        assert_eq!(output.width, 800);
        assert_eq!(output.height, 450);
    }

    #[test]
    fn render_tile_creates_correct_output_dimensions() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        create_test_png(&img_path, 2, 2, &[image::Rgba([0, 0, 0, 255]); 4]);

        let input = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 800,
                height: 450,
            },
            fit: FitMode::Tile,
            background: "#000000".into(),
            opacity: None,
        };

        let output = render_static_image_cpu(input).expect("render");
        assert_eq!(output.width, 800);
        assert_eq!(output.height, 450);
    }

    // --- Error tests ---

    #[test]
    fn invalid_image_path_fails() {
        let input = StaticRenderInput {
            image_path: std::path::PathBuf::from("/nonexistent/path/image.png"),
            viewport: wallflow_package::Viewport {
                width: 100,
                height: 100,
            },
            fit: FitMode::Cover,
            background: "#000000".into(),
            opacity: None,
        };

        let result = render_static_image_cpu(input);
        assert!(result.is_err());
        match result.unwrap_err() {
            StaticRenderError::ImageOpen(_) => {}
            other => panic!("expected ImageOpen error, got: {:?}", other),
        }
    }

    #[test]
    fn invalid_background_fails() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        create_test_png(&img_path, 2, 2, &[image::Rgba([0, 0, 0, 255]); 4]);

        let input = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 100,
                height: 100,
            },
            fit: FitMode::Cover,
            background: "invalid".into(),
            opacity: None,
        };

        let result = render_static_image_cpu(input);
        assert!(result.is_err());
        match result.unwrap_err() {
            StaticRenderError::InvalidBackground(_) => {}
            other => panic!("expected InvalidBackground error, got: {:?}", other),
        }
    }

    // --- Checksum stability ---

    #[test]
    fn checksum_stable() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        create_test_png(&img_path, 2, 2, &[image::Rgba([128, 64, 32, 255]); 4]);

        let input1 = StaticRenderInput {
            image_path: img_path.clone(),
            viewport: wallflow_package::Viewport {
                width: 10,
                height: 10,
            },
            fit: FitMode::Stretch,
            background: "#000000".into(),
            opacity: None,
        };
        let input2 = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 10,
                height: 10,
            },
            fit: FitMode::Stretch,
            background: "#000000".into(),
            opacity: None,
        };

        let output1 = render_static_image_cpu(input1).expect("render1");
        let output2 = render_static_image_cpu(input2).expect("render2");

        assert_eq!(output1.checksum_sha256(), output2.checksum_sha256());
    }

    // --- PNG roundtrip ---

    #[test]
    fn output_png_roundtrip() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        create_test_png(&img_path, 2, 2, &[image::Rgba([255, 0, 0, 255]); 4]);

        let input = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 10,
                height: 10,
            },
            fit: FitMode::Stretch,
            background: "#000000".into(),
            opacity: None,
        };

        let mut output = render_static_image_cpu(input).expect("render");
        let out_path = dir.join("output.png");
        output.save_png(&out_path).expect("save PNG");

        // Reload and verify
        let reloaded = image::open(&out_path).expect("reload PNG");
        assert_eq!(reloaded.width(), 10);
        assert_eq!(reloaded.height(), 10);
    }

    // --- Pixel-level tests ---

    #[test]
    fn stretch_2x2_into_4x4_pixels() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        // 2x2 image: red, green, blue, white
        create_test_png(
            &img_path,
            2,
            2,
            &[
                image::Rgba([255, 0, 0, 255]),     // top-left: red
                image::Rgba([0, 255, 0, 255]),     // top-right: green
                image::Rgba([0, 0, 255, 255]),     // bottom-left: blue
                image::Rgba([255, 255, 255, 255]), // bottom-right: white
            ],
        );

        let input = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 4,
                height: 4,
            },
            fit: FitMode::Stretch,
            background: "#000000".into(),
            opacity: None,
        };

        let output = render_static_image_cpu(input).expect("render");

        // Top-left quadrant should be red-ish (nearest neighbor: stretch 2x2 → 4x4)
        // Pixel (0,0) → source (0,0) = red
        let px = get_pixel(&output, 0, 0);
        assert_eq!(px, [255, 0, 0, 255], "pixel (0,0) should be red");

        // Pixel (3,3) → source (1,1) = white
        let px = get_pixel(&output, 3, 3);
        assert_eq!(px, [255, 255, 255, 255], "pixel (3,3) should be white");
    }

    #[test]
    fn center_2x2_into_4x4_pixels() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        create_test_png(
            &img_path,
            2,
            2,
            &[
                image::Rgba([255, 0, 0, 255]),
                image::Rgba([0, 255, 0, 255]),
                image::Rgba([0, 0, 255, 255]),
                image::Rgba([255, 255, 255, 255]),
            ],
        );

        let input = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 4,
                height: 4,
            },
            fit: FitMode::Center,
            background: "#808080".into(),
            opacity: None,
        };

        let output = render_static_image_cpu(input).expect("render");

        // Center mode: 2x2 image centered in 4x4 viewport
        // dest_rect: x=1, y=1, w=2, h=2
        // Pixel (0,0) should be background (#808080)
        let px = get_pixel(&output, 0, 0);
        assert_eq!(px, [128, 128, 128, 255], "pixel (0,0) should be background");

        // Pixel (1,1) should be red (top-left of centered image)
        let px = get_pixel(&output, 1, 1);
        assert_eq!(px, [255, 0, 0, 255], "pixel (1,1) should be red");
    }

    #[test]
    fn contain_with_background() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        // 2x2 square image into 4x2 viewport (wider than tall)
        // Contain: scale = min(4/2, 2/2) = min(2, 1) = 1
        // dest: w=2, h=2, x=(4-2)/2=1, y=(2-2)/2=0
        create_test_png(
            &img_path,
            2,
            2,
            &[
                image::Rgba([255, 0, 0, 255]),
                image::Rgba([0, 255, 0, 255]),
                image::Rgba([0, 0, 255, 255]),
                image::Rgba([255, 255, 255, 255]),
            ],
        );

        let input = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 4,
                height: 2,
            },
            fit: FitMode::Contain,
            background: "#00ff00".into(),
            opacity: None,
        };

        let output = render_static_image_cpu(input).expect("render");

        // Pixel (0,0) should be green background (left padding)
        let px = get_pixel(&output, 0, 0);
        assert_eq!(
            px,
            [0, 255, 0, 255],
            "pixel (0,0) should be green background"
        );

        // Pixel (1,0) should be red (left edge of contained image)
        let px = get_pixel(&output, 1, 0);
        assert_eq!(px, [255, 0, 0, 255], "pixel (1,0) should be red");
    }

    #[test]
    fn tile_into_5x5_pixels() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        // 2x2 image: each pixel a distinct color
        create_test_png(
            &img_path,
            2,
            2,
            &[
                image::Rgba([255, 0, 0, 255]),     // (0,0) red
                image::Rgba([0, 255, 0, 255]),     // (1,0) green
                image::Rgba([0, 0, 255, 255]),     // (0,1) blue
                image::Rgba([255, 255, 255, 255]), // (1,1) white
            ],
        );

        let input = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 5,
                height: 5,
            },
            fit: FitMode::Tile,
            background: "#000000".into(),
            opacity: None,
        };

        let output = render_static_image_cpu(input).expect("render");

        // (0,0) = red (tile starts from 0,0)
        let px = get_pixel(&output, 0, 0);
        assert_eq!(px, [255, 0, 0, 255], "tile (0,0) should be red");

        // (2,0) = red (wraps: 2 % 2 = 0)
        let px = get_pixel(&output, 2, 0);
        assert_eq!(px, [255, 0, 0, 255], "tile (2,0) should be red (wrap)");

        // (1,1) = white (1 % 2 = 1, 1 % 2 = 1 → source (1,1))
        let px = get_pixel(&output, 1, 1);
        assert_eq!(px, [255, 255, 255, 255], "tile (1,1) should be white");

        // (4,4) = white (4 % 2 = 0→source x=0, 4 % 2 = 0→source y=0) → wait, 4%2=0, so source (0,0) = red
        let px = get_pixel(&output, 4, 4);
        assert_eq!(px, [255, 0, 0, 255], "tile (4,4) should be red (wrap)");
    }

    /// Helper: get the RGBA pixel at (x, y) from a RenderOutput.
    fn get_pixel(output: &RenderOutput, x: u32, y: u32) -> [u8; 4] {
        let idx = ((y * output.width + x) * 4) as usize;
        [
            output.pixels_rgba[idx],
            output.pixels_rgba[idx + 1],
            output.pixels_rgba[idx + 2],
            output.pixels_rgba[idx + 3],
        ]
    }

    #[test]
    fn render_output_metadata() {
        let dir = unique_test_dir();
        let img_path = dir.join("test.png");
        create_test_png(&img_path, 2, 2, &[image::Rgba([0, 0, 0, 255]); 4]);

        let input = StaticRenderInput {
            image_path: img_path,
            viewport: wallflow_package::Viewport {
                width: 10,
                height: 10,
            },
            fit: FitMode::Stretch,
            background: "#000000".into(),
            opacity: None,
        };

        let mut output = render_static_image_cpu(input).expect("render");
        let out_path = dir.join("meta_test.png");
        output.save_png(&out_path).expect("save");

        let meta = output.metadata();
        assert_eq!(meta.width, 10);
        assert_eq!(meta.height, 10);
        assert_eq!(meta.pixel_format, "RGBA8");
        assert!(meta.file_size_bytes.is_some());
        assert!(meta.checksum.is_some());
    }
}
