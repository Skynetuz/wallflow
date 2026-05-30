# 011 — Static Render Output

**Status**: Implemented (Stage 008)
**Crate**: `wallflow-render`
**Depends on**: `wallflow-common`, `wallflow-package`

## Overview

The static render output layer produces actual RGBA pixel data from a decoded image, layout, fit mode, and background color. This is the first real render output in the WallFlow pipeline — a CPU reference renderer that runs without any GPU, display server, or window manager, making it fully cloud-testable.

The CPU reference renderer serves as the baseline before any wgpu GPU presentation is added. Every fit mode produces deterministic, verifiable output that can be tested in CI on Linux without a display server.

## Why a CPU Reference Renderer First

Before investing in wgpu presentation (which requires a GPU context, surface configuration, and platform-specific window integration), a CPU renderer provides several critical benefits:

1. **Cloud-testable validation**: The entire render pipeline can be exercised in CI on Ubuntu runners with no GPU or display server. Every fit mode, background fill, and layout calculation produces actual pixel output that can be verified.

2. **Deterministic reference output**: The CPU renderer produces byte-identical output for the same input. This enables SHA-256 checksums for snapshot-like tests without storing large binary fixtures.

3. **Architecture validation**: The render pipeline (input types → layout → pixel output → PNG) can be validated end-to-end before the complexity of GPU shaders and surface management is introduced.

4. **Documentation of intent**: The CPU renderer makes the exact pixel-level behavior of each fit mode explicit in straightforward Rust code. This serves as a specification for the wgpu implementation.

## Architecture

```
┌──────────────────────┐
│  StaticRenderInput   │  image_path, viewport, fit, background, opacity
└──────────┬───────────┘
           │
           ▼
┌──────────────────────┐
│ render_static_image  │  CPU reference renderer
│       _cpu()         │
└──────────┬───────────┘
           │
     ┌─────┴─────┐
     │            │
     ▼            ▼
┌─────────┐  ┌──────────────────┐
│ Layout  │  │ Pixel compositing│
│ Engine  │  │ (fit + bg + img) │
└────┬────┘  └────────┬─────────┘
     │                │
     └───────┬────────┘
             ▼
    ┌────────────────┐
    │  RenderOutput   │  width, height, pixels_rgba, output_path
    └────────┬───────┘
             │
     ┌───────┴────────┐
     │                 │
     ▼                 ▼
┌──────────┐   ┌───────────────────┐
│ save_png │   │ checksum_sha256()  │
└──────────┘   │ metadata()        │
               └───────────────────┘
```

## Key Types

### RenderBackend

```rust
pub enum RenderBackend {
    CpuReference,
    // WgpuExperimental — future stage
}
```

Prepared for multiple backends. The CPU reference renderer is the only implementation today. When wgpu is added later, the same `StaticRenderInput` flows through both paths.

### StaticRenderInput

```rust
pub struct StaticRenderInput {
    pub image_path: PathBuf,
    pub viewport: Viewport,
    pub fit: FitMode,
    pub background: String,  // hex color like "#000000"
    pub opacity: Option<u8>,
}
```

The input contract for static image rendering. The `background` field accepts hex color strings parsed by `RgbaColor::parse_hex()`. The `viewport` uses the same `Viewport` type from the layout engine.

### RenderOutput

```rust
pub struct RenderOutput {
    pub width: u32,
    pub height: u32,
    pub pixels_rgba: Vec<u8>,
    pub output_path: Option<PathBuf>,
}
```

The pixel buffer output. Methods include `save_png()` for writing to disk, `checksum_sha256()` for stable checksums, and `metadata()` for structured output information.

### RenderOutputMetadata

```rust
pub struct RenderOutputMetadata {
    pub width: u32,
    pub height: u32,
    pub pixel_format: String,       // "RGBA8"
    pub file_size_bytes: Option<u64>,
    pub checksum: Option<String>,   // SHA-256 hex
}
```

Serializable metadata about the render output. The checksum is over the raw RGBA pixel data, not the PNG file, making it stable regardless of PNG compression settings.

### RgbaColor

```rust
pub struct RgbaColor {
    pub r: u8, pub g: u8, pub b: u8, pub a: u8,
}
```

Safe parser for hex color strings. Supports `#RRGGBB` and `#RRGGBBAA` formats. Rejects missing `#`, wrong lengths, and invalid hex digits with clear error messages.

### StaticRenderError

Error enum covering invalid image paths, decode failures, invalid backgrounds, invalid viewports, layout errors, and I/O errors. All errors flow through `Result` — no `unwrap()` or `panic!()`.

## Fit Mode Behavior

Each fit mode determines how the source image is placed within the viewport:

| Mode | Scaling | Positioning | Clipping | Background |
|------|---------|-------------|----------|------------|
| **Cover** | Scale up to fill viewport | Centered | Image may extend beyond edges | Not visible (image covers all) |
| **Contain** | Scale up to fit within viewport | Centered | No clipping | Visible in letterbox/pillarbox bars |
| **Stretch** | Distort to exact viewport size | Top-left origin | No clipping | Not visible (image fills exactly) |
| **Center** | No scaling | Centered | No clipping | Visible around image |
| **Tile** | No scaling | Repeated from top-left | No clipping | Not visible (tiles fill viewport) |

### Cover

The image is scaled by the largest factor that makes it at least as large as the viewport in both dimensions, then centered. Parts of the image that extend beyond the viewport are clipped. The background is fully covered.

### Contain

The image is scaled by the smallest factor that makes it fit entirely within the viewport, then centered. Background is visible in the letterbox (horizontal) or pillarbox (vertical) bars.

### Stretch

The image is distorted to exactly match the viewport dimensions. No background is visible. This mode does not preserve aspect ratio.

### Center

The image is placed at its native resolution centered in the viewport. No scaling occurs. Background is visible around the image. If the image is larger than the viewport, the overflow is clipped.

### Tile

The image is repeated at its native resolution starting from the top-left corner, filling the entire viewport. No scaling occurs. Background is not visible.

## Scaling Method

The current implementation uses **nearest-neighbor scaling**. This is a deliberate temporary choice:

- Nearest-neighbor is deterministic and produces pixel-identical output across all platforms.
- It is simple to implement correctly in pure Rust without SIMD or GPU.
- It makes pixel-level assertions in tests straightforward.

**Future improvement**: Bilinear or Lanczos scaling can be added as an option, likely in the wgpu backend. The CPU reference renderer should continue to offer nearest-neighbor as the baseline.

## Integration with Renderer Process

The `--headless-render-sim` mode of `wallflow-renderer` now supports actual pixel output:

```bash
cargo run -p wallflow-renderer -- \
  --headless-render-sim \
  --width 800 --height 450 \
  --source input.png \
  --render-output output.png \
  --fit cover
```

When `--render-output` is specified:
1. The image is decoded from `--source`.
2. Layout is calculated for the given viewport and fit mode.
3. The CPU renderer produces RGBA pixel data.
4. The output is saved as PNG to `--render-output`.
5. A structured JSON report is printed to stdout including output dimensions, checksum, layout, and source metadata.

When `--render-output` is not specified, the renderer stays in layout-only simulation mode (backward compatible with stage 007 behavior).

## CLI Smoke Test

The `render-output-smoke` command validates the full render pipeline:

```bash
cargo run -p wallflow-cli -- render-output-smoke
```

This:
1. Creates a 2×2 test PNG with distinct pixel colors.
2. Runs the renderer in `--headless-render-sim` mode with `--render-output`.
3. Verifies the output PNG exists and decodes correctly.
4. Verifies output dimensions match the viewport (800×450).
5. Verifies a SHA-256 checksum is present in the report.
6. Outputs a structured JSON report.

This runs in CI on Ubuntu without any display server.

## Relationship to Future wgpu Backend

The CPU reference renderer and the future wgpu backend share the same input types (`StaticRenderInput`) and output types (`RenderOutput`). The `RenderBackend` enum is prepared for this:

```rust
match backend {
    RenderBackend::CpuReference => render_static_image_cpu(input),
    // RenderBackend::WgpuExperimental => render_static_image_wgpu(input, device, surface),
}
```

The wgpu backend will produce the same RGBA pixel data (verified against the CPU reference) but using GPU shaders for performance. The CPU reference remains available for testing and as a fallback.

## What Remains REQUIRES_REAL_WINDOWS_VALIDATION

- Desktop attach behind desktop icons (WorkerW/Progman)
- Real Explorer window integration
- Multi-monitor layout
- DPI scaling with actual display
- Fullscreen detection on Windows
- GPU surface rendering on Windows Desktop Window Manager

These require an interactive Windows session with a real display and cannot be tested in CI.
