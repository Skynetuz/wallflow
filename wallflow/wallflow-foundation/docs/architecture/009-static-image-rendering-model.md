# Static Image Rendering Model

## Overview

The static image rendering model defines how a single image file is transformed
into a wallpaper on a display viewport. It consists of two phases:

1. **Image Decode** — Read the image file and extract metadata (dimensions, format,
   color type) without fully decoding all pixels.
2. **Layout Calculation** — Given the image dimensions, the viewport dimensions,
   and a fit mode, compute the destination rectangle where the image should be
   drawn.

## Image Decode

The `wallflow-package` crate provides `load_image_metadata()` which uses
`image::io::Reader` to read image dimensions and format information without
fully decoding all pixels. This is efficient for large images since only the
header is read.

### Metadata

```rust
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub color_type: String,
    pub detected_format: String,
    pub file_size_bytes: u64,
}
```

- `color_type` is the stringified `image::ColorType` (e.g., "Rgba8", "Rgb8").
- `detected_format` is the stringified `image::ImageFormat` (e.g., "Png", "Jpeg").

## Layout Calculation

The `calculate_static_image_layout()` function in `wallflow-package::layout`
computes how an image should be positioned and scaled within a viewport.

### Viewport

The viewport is a synthetic concept representing the display area. In the
current cloud-testable implementation, a default viewport of 1920×1080 is used.
In the future, this will come from actual monitor information.

```rust
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}
```

### Fit Modes

Five fit modes are supported, each with a specific formula:

#### Cover

Scale the image uniformly to fill the entire viewport. One dimension will
match exactly, the other will overflow (be cropped).

```
scale = max(viewport_width / image_width, viewport_height / image_height)
dest_width = image_width × scale
dest_height = image_height × scale
x = (viewport_width - dest_width) / 2
y = (viewport_height - dest_height) / 2
```

The destination rectangle may extend beyond the viewport boundaries (negative
x or y, or width/height exceeding viewport dimensions). The renderer should
clip to the viewport.

#### Contain

Scale the image uniformly to fit entirely within the viewport. One dimension
will match exactly, the other will have letterboxing (empty space).

```
scale = min(viewport_width / image_width, viewport_height / image_height)
dest_width = image_width × scale
dest_height = image_height × scale
x = (viewport_width - dest_width) / 2
y = (viewport_height - dest_height) / 2
```

The destination rectangle will always be within the viewport. The background
color fills the letterboxed area.

#### Stretch

Scale non-uniformly to exactly fill the viewport. Both dimensions match
the viewport exactly.

```
dest_width = viewport_width
dest_height = viewport_height
x = 0
y = 0
```

This may distort the image aspect ratio.

#### Center

No scaling. Center the image at its natural size.

```
dest_width = image_width
dest_height = image_height
x = (viewport_width - image_width) / 2
y = (viewport_height - image_height) / 2
```

The image may be smaller than the viewport (letterboxing) or larger
(overflow/clip).

#### Tile

Repeat the image at its natural size to cover the entire viewport.

```
tile_size = (image_width, image_height)
destination = (0, 0, viewport_width, viewport_height)
```

The `tile_size` field in `StaticImageLayout` is set to the image dimensions,
indicating the tile repeat interval. The `destination_rect` covers the full
viewport.

### Zero Dimensions

If any dimension (image or viewport) is zero, `LayoutError::ZeroDimension` is
returned. Zero dimensions are never valid for layout calculation.

## Layout Report (IPC)

The layout calculation result is transmitted over IPC as a
`StaticImageLayoutReport`:

```rust
pub struct StaticImageLayoutReport {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub image_width: u32,
    pub image_height: u32,
    pub fit: FitMode,
    pub destination_x: f64,
    pub destination_y: f64,
    pub destination_width: f64,
    pub destination_height: f64,
    pub background: String,
}
```

This is an IPC-safe type (no dependency on the `image` crate) that can be
sent from the renderer process to Core.

## Synthetic Viewport

In the current implementation, the renderer uses a synthetic viewport of
1920×1080 when calculating layout. This is because:

1. The renderer does not have access to real monitor information in the
   cloud-testable (Linux) environment.
2. The layout calculation is tested independently of actual display hardware.

When real Windows desktop integration is implemented, the viewport will be
sourced from `MonitorInfo.size_px`.

## Render Output (CPU Reference)

The layout calculation described above determines *where* and *how large* the image should be drawn, but does not produce actual pixels. The render output layer in `wallflow-render` takes the layout result and composites real RGBA pixel data.

For full details, see `docs/architecture/011-static-render-output.md`.

The render pipeline is:

1. **Image Decode** — `load_image_metadata()` reads dimensions (as above).
2. **Full Pixel Decode** — `image::open()` loads the full pixel data and converts to RGBA8.
3. **Layout Calculation** — `calculate_static_image_layout()` computes the destination rectangle (as above).
4. **Pixel Compositing** — `render_static_image_cpu()` fills the viewport buffer with the background color, then draws the image according to the fit mode using nearest-neighbor scaling.
5. **Output** — The resulting `RenderOutput` contains the RGBA pixel buffer, and can be saved as PNG or checksummed with SHA-256.

The CPU reference renderer is fully cloud-testable. It validates that each fit mode produces the correct pixel output, including background fill, clipping, and tiling, without requiring a GPU or display server.

## REQUIRES_REAL_WINDOWS_VALIDATION

The following items require validation on a real Windows environment:

- Layout with actual monitor dimensions (not synthetic 1920×1080)
- Visual correctness of each fit mode on a real display (cover cropping, contain letterboxing,
  stretch distortion, center positioning, tile tiling)
- Background color rendering in contain/center modes
- Clip behavior when destination rect extends beyond viewport
- GPU-rendered output matching the CPU reference output
