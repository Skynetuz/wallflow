use serde::{Deserialize, Serialize};
use thiserror::Error;
use wallflow_common::FitMode;

/// Viewport dimensions for layout calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

/// Image dimensions for layout calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

/// A rectangle in pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RenderRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The calculated layout for a static image wallpaper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StaticImageLayout {
    pub viewport: Viewport,
    pub image_size: ImageSize,
    pub fit: FitMode,
    pub destination_rect: RenderRect,
    pub tile_size: Option<ImageSize>,
    pub background: String,
}

/// Error in layout calculation.
#[derive(Debug, Error)]
pub enum LayoutError {
    #[error("zero dimension not allowed: width={width}, height={height}")]
    ZeroDimension { width: u32, height: u32 },
}

/// Calculate how a static image should be laid out in a viewport.
///
/// Returns a `StaticImageLayout` describing the destination rectangle
/// and any additional layout information (e.g., tile size for tile mode).
pub fn calculate_static_image_layout(
    image_size: ImageSize,
    viewport: Viewport,
    fit: FitMode,
    background: String,
) -> Result<StaticImageLayout, LayoutError> {
    if image_size.width == 0
        || image_size.height == 0
        || viewport.width == 0
        || viewport.height == 0
    {
        return Err(LayoutError::ZeroDimension {
            width: image_size.width,
            height: image_size.height,
        });
    }

    let iw = f64::from(image_size.width);
    let ih = f64::from(image_size.height);
    let vw = f64::from(viewport.width);
    let vh = f64::from(viewport.height);

    let (destination_rect, tile_size) = match fit {
        FitMode::Cover => {
            let scale = (vw / iw).max(vh / ih);
            let dest_w = iw * scale;
            let dest_h = ih * scale;
            let x = (vw - dest_w) / 2.0;
            let y = (vh - dest_h) / 2.0;
            (
                RenderRect {
                    x,
                    y,
                    width: dest_w,
                    height: dest_h,
                },
                None,
            )
        }
        FitMode::Contain => {
            let scale = (vw / iw).min(vh / ih);
            let dest_w = iw * scale;
            let dest_h = ih * scale;
            let x = (vw - dest_w) / 2.0;
            let y = (vh - dest_h) / 2.0;
            (
                RenderRect {
                    x,
                    y,
                    width: dest_w,
                    height: dest_h,
                },
                None,
            )
        }
        FitMode::Stretch => (
            RenderRect {
                x: 0.0,
                y: 0.0,
                width: vw,
                height: vh,
            },
            None,
        ),
        FitMode::Center => {
            let x = (vw - iw) / 2.0;
            let y = (vh - ih) / 2.0;
            (
                RenderRect {
                    x,
                    y,
                    width: iw,
                    height: ih,
                },
                None,
            )
        }
        FitMode::Tile => (
            RenderRect {
                x: 0.0,
                y: 0.0,
                width: vw,
                height: vh,
            },
            Some(image_size),
        ),
    };

    Ok(StaticImageLayout {
        viewport,
        image_size,
        fit,
        destination_rect,
        tile_size,
        background,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wallflow_common::FitMode;

    const BG: &str = "#000000";

    #[test]
    fn cover_landscape_image_into_landscape_viewport() {
        // 4:3 image into 16:9 viewport — scale fills 16:9
        let image = ImageSize {
            width: 4,
            height: 3,
        };
        let viewport = Viewport {
            width: 16,
            height: 9,
        };
        let layout = calculate_static_image_layout(image, viewport, FitMode::Cover, BG.to_string())
            .expect("layout");
        // scale = max(16/4, 9/3) = max(4, 3) = 4
        // dest_w = 4*4 = 16, dest_h = 3*4 = 12
        // x = (16-16)/2 = 0, y = (9-12)/2 = -1.5
        assert_eq!(layout.destination_rect.width, 16.0);
        assert_eq!(layout.destination_rect.height, 12.0);
        assert_eq!(layout.destination_rect.x, 0.0);
        assert_eq!(layout.destination_rect.y, -1.5);
        assert!(layout.tile_size.is_none());
    }

    #[test]
    fn contain_landscape_image_into_landscape_viewport() {
        // 4:3 image into 16:9 viewport — scale fits entirely
        let image = ImageSize {
            width: 4,
            height: 3,
        };
        let viewport = Viewport {
            width: 16,
            height: 9,
        };
        let layout =
            calculate_static_image_layout(image, viewport, FitMode::Contain, BG.to_string())
                .expect("layout");
        // scale = min(16/4, 9/3) = min(4, 3) = 3
        // dest_w = 4*3 = 12, dest_h = 3*3 = 9
        // x = (16-12)/2 = 2, y = (9-9)/2 = 0
        assert_eq!(layout.destination_rect.width, 12.0);
        assert_eq!(layout.destination_rect.height, 9.0);
        assert_eq!(layout.destination_rect.x, 2.0);
        assert_eq!(layout.destination_rect.y, 0.0);
    }

    #[test]
    fn stretch_fills_viewport_exactly() {
        let image = ImageSize {
            width: 4,
            height: 3,
        };
        let viewport = Viewport {
            width: 16,
            height: 9,
        };
        let layout =
            calculate_static_image_layout(image, viewport, FitMode::Stretch, BG.to_string())
                .expect("layout");
        assert_eq!(layout.destination_rect.x, 0.0);
        assert_eq!(layout.destination_rect.y, 0.0);
        assert_eq!(layout.destination_rect.width, 16.0);
        assert_eq!(layout.destination_rect.height, 9.0);
    }

    #[test]
    fn center_no_scaling() {
        let image = ImageSize {
            width: 800,
            height: 600,
        };
        let viewport = Viewport {
            width: 1920,
            height: 1080,
        };
        let layout =
            calculate_static_image_layout(image, viewport, FitMode::Center, BG.to_string())
                .expect("layout");
        assert_eq!(layout.destination_rect.width, 800.0);
        assert_eq!(layout.destination_rect.height, 600.0);
        assert_eq!(layout.destination_rect.x, (1920.0 - 800.0) / 2.0);
        assert_eq!(layout.destination_rect.y, (1080.0 - 600.0) / 2.0);
    }

    #[test]
    fn tile_returns_tile_size() {
        let image = ImageSize {
            width: 256,
            height: 256,
        };
        let viewport = Viewport {
            width: 1920,
            height: 1080,
        };
        let layout = calculate_static_image_layout(image, viewport, FitMode::Tile, BG.to_string())
            .expect("layout");
        assert_eq!(layout.destination_rect.x, 0.0);
        assert_eq!(layout.destination_rect.y, 0.0);
        assert_eq!(layout.destination_rect.width, 1920.0);
        assert_eq!(layout.destination_rect.height, 1080.0);
        assert_eq!(
            layout.tile_size,
            Some(ImageSize {
                width: 256,
                height: 256
            })
        );
    }

    #[test]
    fn zero_width_rejects() {
        let image = ImageSize {
            width: 0,
            height: 100,
        };
        let viewport = Viewport {
            width: 1920,
            height: 1080,
        };
        let result = calculate_static_image_layout(image, viewport, FitMode::Cover, BG.to_string());
        assert!(result.is_err());
        match result {
            Err(LayoutError::ZeroDimension { width, height }) => {
                assert_eq!(width, 0);
                assert_eq!(height, 100);
            }
            _ => panic!("expected ZeroDimension error"),
        }
    }

    #[test]
    fn zero_height_rejects() {
        let image = ImageSize {
            width: 100,
            height: 0,
        };
        let viewport = Viewport {
            width: 1920,
            height: 1080,
        };
        let result = calculate_static_image_layout(image, viewport, FitMode::Cover, BG.to_string());
        assert!(result.is_err());
    }

    #[test]
    fn zero_viewport_dimension_rejects() {
        let image = ImageSize {
            width: 100,
            height: 100,
        };
        let viewport = Viewport {
            width: 0,
            height: 1080,
        };
        let result = calculate_static_image_layout(image, viewport, FitMode::Cover, BG.to_string());
        assert!(result.is_err());
    }

    #[test]
    fn same_size_image_and_viewport_cover() {
        // 1920x1080 image into 1920x1080 viewport
        let image = ImageSize {
            width: 1920,
            height: 1080,
        };
        let viewport = Viewport {
            width: 1920,
            height: 1080,
        };
        let layout = calculate_static_image_layout(image, viewport, FitMode::Cover, BG.to_string())
            .expect("layout");
        // scale = max(1, 1) = 1, so exact fit
        assert_eq!(layout.destination_rect.width, 1920.0);
        assert_eq!(layout.destination_rect.height, 1080.0);
        assert_eq!(layout.destination_rect.x, 0.0);
        assert_eq!(layout.destination_rect.y, 0.0);
    }

    #[test]
    fn four_three_image_into_sixteen_nine_cover() {
        let image = ImageSize {
            width: 1024,
            height: 768,
        };
        let viewport = Viewport {
            width: 1920,
            height: 1080,
        };
        let layout = calculate_static_image_layout(image, viewport, FitMode::Cover, BG.to_string())
            .expect("layout");
        // scale = max(1920/1024, 1080/768) = max(1.875, 1.40625) = 1.875
        // dest_w = 1024*1.875 = 1920, dest_h = 768*1.875 = 1440
        // x = (1920-1920)/2 = 0, y = (1080-1440)/2 = -180
        assert_eq!(layout.destination_rect.width, 1920.0);
        assert!((layout.destination_rect.height - 1440.0).abs() < 0.001);
        assert_eq!(layout.destination_rect.x, 0.0);
        assert!((layout.destination_rect.y - (-180.0)).abs() < 0.001);
    }

    #[test]
    fn four_three_image_into_sixteen_nine_contain() {
        let image = ImageSize {
            width: 1024,
            height: 768,
        };
        let viewport = Viewport {
            width: 1920,
            height: 1080,
        };
        let layout =
            calculate_static_image_layout(image, viewport, FitMode::Contain, BG.to_string())
                .expect("layout");
        // scale = min(1920/1024, 1080/768) = min(1.875, 1.40625) = 1.40625
        // dest_w = 1024*1.40625 = 1440, dest_h = 768*1.40625 = 1080
        // x = (1920-1440)/2 = 240, y = (1080-1080)/2 = 0
        assert!((layout.destination_rect.width - 1440.0).abs() < 0.001);
        assert!((layout.destination_rect.height - 1080.0).abs() < 0.001);
        assert!((layout.destination_rect.x - 240.0).abs() < 0.001);
        assert!((layout.destination_rect.y).abs() < 0.001);
    }

    #[test]
    fn portrait_image_into_landscape_viewport_cover() {
        // 9:16 portrait image into 16:9 landscape viewport
        let image = ImageSize {
            width: 1080,
            height: 1920,
        };
        let viewport = Viewport {
            width: 1920,
            height: 1080,
        };
        let layout = calculate_static_image_layout(image, viewport, FitMode::Cover, BG.to_string())
            .expect("layout");
        // scale = max(1920/1080, 1080/1920) = max(1.7778, 0.5625) = 1.7778
        // dest_w = 1080*1.7778 = 1920, dest_h = 1920*1.7778 = 3413.33
        assert!((layout.destination_rect.width - 1920.0).abs() < 0.01);
        assert!(layout.destination_rect.height > 3000.0);
    }

    #[test]
    fn portrait_image_into_landscape_viewport_contain() {
        let image = ImageSize {
            width: 1080,
            height: 1920,
        };
        let viewport = Viewport {
            width: 1920,
            height: 1080,
        };
        let layout =
            calculate_static_image_layout(image, viewport, FitMode::Contain, BG.to_string())
                .expect("layout");
        // scale = min(1920/1080, 1080/1920) = min(1.7778, 0.5625) = 0.5625
        // dest_w = 1080*0.5625 = 607.5, dest_h = 1920*0.5625 = 1080
        assert!((layout.destination_rect.height - 1080.0).abs() < 0.01);
        assert!(layout.destination_rect.width < 700.0);
    }

    #[test]
    fn layout_preserves_fit_mode() {
        let image = ImageSize {
            width: 100,
            height: 100,
        };
        let viewport = Viewport {
            width: 200,
            height: 200,
        };
        let layout =
            calculate_static_image_layout(image, viewport, FitMode::Contain, BG.to_string())
                .expect("layout");
        assert_eq!(layout.fit, FitMode::Contain);
    }

    #[test]
    fn layout_preserves_background() {
        let image = ImageSize {
            width: 100,
            height: 100,
        };
        let viewport = Viewport {
            width: 200,
            height: 200,
        };
        let layout =
            calculate_static_image_layout(image, viewport, FitMode::Cover, "#1a1a2e".to_string())
                .expect("layout");
        assert_eq!(layout.background, "#1a1a2e");
    }
}
