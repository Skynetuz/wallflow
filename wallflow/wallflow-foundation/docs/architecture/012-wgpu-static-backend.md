# 012 — wgpu Static Backend Skeleton

**Status**: Implemented (Stage 009)
**Crate**: `wallflow-render`
**Depends on**: `wallflow-common`, `wallflow-package`, `wgpu`, `pollster`
**Precedes**: Stage 010 (textured quad rendering)

## Overview

The wgpu static backend skeleton introduces an **experimental GPU render path** alongside the existing CPU reference renderer. Its purpose is to establish the infrastructure for GPU-accelerated static image rendering without yet completing the full shader pipeline.

Key design goals:

1. **Scaffold, not production**: The wgpu backend is a clear-only experimental path today. It proves that the wgpu instance/adapter/device lifecycle works, that textures can be created and cleared, and that pixel data can be read back to the CPU. It does NOT yet render textured quads.

2. **Graceful degradation**: If no GPU adapter is available (the common case in headless CI), the wgpu backend returns structured errors — it never panics, never blocks, and never breaks the CPU reference path.

3. **CPU reference remains primary**: The `CpuReference` backend is the default and the only path used in CI. The `WgpuExperimental` backend is opt-in via the `--backend wgpu` flag. The CPU renderer continues to serve as the cloud-testable, deterministic baseline.

4. **Testability**: The capability probe (`probe_wgpu_capabilities()`) and the smoke test (`wgpu-smoke`) both always exit with code 0, even when no GPU is present. This ensures they can run in any CI environment without special GPU runners.

## Architecture

```
                         ┌───────────────────────────────────────────────────┐
                         │              wallflow-renderer                    │
                         │         (--headless-render-sim mode)              │
                         └─────────────────────┬─────────────────────────────┘
                                               │
                                               │  --backend {cpu|wgpu}
                                               │
                    ┌──────────────────────────┼───────────────────────────┐
                    │                          │                           │
                    ▼                          ▼                           │
        ┌───────────────────┐    ┌─────────────────────────┐               │
        │   CpuReference    │    │   WgpuExperimental      │               │
        │   (default)       │    │   (opt-in)              │               │
        └─────────┬─────────┘    └────────────┬────────────┘               │
                  │                           │                            │
                  │                           │                            │
    ┌─────────────▼──────────────┐ ┌──────────▼──────────────────────────┐ │
    │ render_static_image_cpu()  │ │ probe_wgpu_capabilities()          │ │
    │                            │ │ render_static_image_wgpu_offscreen │ │
    │ • decode image (image crate)│ │                                    │ │
    │ • layout engine            │ │ • instance → adapter → device      │ │
    │ • nearest-neighbor scale   │ │ • create output texture            │ │
    │ • alpha blend compositing  │ │ • clear pass with bg color        │ │
    │ • all fit modes            │ │ • copy texture → CPU buffer        │ │
    │ • PNG output               │ │ • return RenderOutput              │ │
    │ • SHA-256 checksum         │ │ • NO image texture upload (yet)    │ │
    └─────────────┬──────────────┘ └──────────┬──────────────────────────┘ │
                  │                           │                            │
                  └───────────┬───────────────┘                            │
                              ▼                                            │
                    ┌──────────────────┐                                   │
                    │   RenderOutput   │  width, height, pixels_rgba,      │
                    │                  │  output_path, checksum_sha256()    │
                    └──────────────────┘                                   │
                                                                           │
    ┌──────────────────────────────────────────────────────────────────────┘
    │
    │   CLI Commands
    │
    ├─ wallflow wgpu-probe     → probe_wgpu_capabilities() → JSON report
    ├─ wallflow wgpu-smoke     → probe + offscreen clear render → JSON report
    └─ wallflow-renderer --headless-render-sim --backend wgpu
                                  → clear-only offscreen render → PNG + JSON
```

## Key Types

### RenderBackend

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RenderBackend {
    #[default]
    CpuReference,
    WgpuExperimental,
}
```

The `RenderBackend` enum selects which rendering path to use. It is:

- **`CpuReference`** (default): Pure Rust, no GPU, no display server, cloud-testable. This is the path used in all CI pipelines.
- **`WgpuExperimental`**: Requires a GPU adapter. May be unavailable in headless environments. Currently produces clear-only output (no image texture).

The enum implements `Display` (`"cpu"` / `"wgpu"`) and `FromStr` (case-insensitive: `"cpu"`, `"wgpu"`, `"WGPU"` all work), making it suitable for CLI flag parsing and serialization.

When `WgpuExperimental` is selected but no adapter is found, the render function returns `WgpuRenderError::NoAdapter` — the caller must fall back to `CpuReference` or report the error gracefully.

### WgpuRenderCapabilities

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WgpuRenderCapabilities {
    pub supported: bool,
    pub adapter_name: Option<String>,
    pub backend: Option<String>,       // "Vulkan", "Metal", "Dx12", "Gl"
    pub device_type: Option<String>,   // "DiscreteGpu", "IntegratedGpu", "Cpu"
    pub features: Vec<String>,
    pub limits: Option<WgpuLimitsSummary>,
    pub failure_reason: Option<String>,
}
```

Returned by `probe_wgpu_capabilities()`. Fully serializable to JSON for CLI output and diagnostics. When `supported` is `false`, `failure_reason` explains why (e.g., "no adapter available: NotFound" or "device creation failed: ...").

The `features` list contains all wgpu features reported by the device as debug-format strings (e.g., `"TextureCompressionBc"`). This is useful for future capability-gated code paths.

### WgpuLimitsSummary

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WgpuLimitsSummary {
    pub max_texture_dimension_2d: u32,
    pub max_buffer_size: u64,
}
```

A curated subset of GPU limits relevant to static image rendering. The full `wgpu::Limits` struct has dozens of fields; only the two most critical for wallpaper rendering are exposed:

- **`max_texture_dimension_2d`**: The maximum width/height of a 2D texture. Wallpapers at 4K (3840×2160) or 8K (7680×4320) must fit within this limit. Typical values: 16384 on modern GPUs, 4096 on older/integrated.
- **`max_buffer_size`**: The maximum size of a GPU buffer in bytes. This limits the size of the readback buffer when copying texture data to CPU memory.

### WgpuRenderError

```rust
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
```

All wgpu backend errors flow through this enum. The `NotImplemented` variant is reserved for future operations (like textured quad rendering) that are not yet wired up. Every variant includes a descriptive message — no opaque error codes.

The `NoAdapter` variant is the most common error in CI. It is **expected** in headless Linux environments and should be handled gracefully, not treated as a fatal error.

## Capability Probe: `probe_wgpu_capabilities()`

### How it works

```rust
pub fn probe_wgpu_capabilities() -> WgpuRenderCapabilities
```

The probe follows this sequence:

1. **Create wgpu Instance**: Uses `wgpu::Instance::new()` with `Backends::all()` to try all available graphics APIs (Vulkan, Metal, Dx12, OpenGL). No display or surface is required.

2. **Request Adapter**: Calls `instance.request_adapter()` with `force_fallback_adapter: true` and `compatible_surface: None`. The fallback flag ensures that even a software/CPU adapter (like llvmpipe or Vulkan software rasterizer) will be returned if available. Setting `compatible_surface: None` means the adapter does not need to support rendering to a window surface.

3. **If no adapter**: Returns immediately with `supported: false` and a descriptive `failure_reason`. This is the expected path in headless CI without GPU drivers.

4. **Request Device**: If an adapter is found, calls `adapter.request_device()` with `required_features: empty()` and `required_limits: downlevel_webgl2_defaults()`. These are the most conservative limits, ensuring the device can be created even on older or integrated GPUs.

5. **Collect information**: If device creation succeeds, reports the adapter name, backend type, device type, feature list, and key limits.

6. **Return structured report**: The result is a `WgpuRenderCapabilities` with `supported: true` (or `false` with reason).

### Safe in headless environments

The probe is designed to be safe in any environment:

- **No panic guarantee**: If wgpu initialization fails (no drivers, no GPU, no Vulkan/Metal/Dx12/OpenGL), the function returns `supported: false` with a reason string. It never panics.
- **No display server required**: `compatible_surface: None` means the probe does not need a Wayland compositor, X11 server, or Windows desktop.
- **Fast**: The probe takes milliseconds. It creates and drops GPU resources immediately.
- **Always exit 0**: The `wgpu-probe` CLI command always exits with code 0, even when no GPU is found. It is a diagnostic tool, not a pass/fail test.

### Typical output

On a machine with a discrete GPU:
```json
{
  "supported": true,
  "adapter_name": "NVIDIA GeForce RTX 3080",
  "backend": "Vulkan",
  "device_type": "DiscreteGpu",
  "features": ["TextureCompressionBc", "TimestampQuery", ...],
  "limits": {
    "max_texture_dimension_2d": 16384,
    "max_buffer_size": 4294967296
  },
  "failure_reason": null
}
```

In headless CI (no GPU):
```json
{
  "supported": false,
  "adapter_name": null,
  "backend": null,
  "device_type": null,
  "features": [],
  "limits": null,
  "failure_reason": "no adapter available: NotFound"
}
```

## Offscreen Render Path: `render_static_image_wgpu_offscreen()`

### Current state: clear-only experimental

```rust
pub fn render_static_image_wgpu_offscreen(
    width: u32,
    height: u32,
    background: &str,
) -> Result<RenderOutput, WgpuRenderError>
```

This function creates an offscreen wgpu render pipeline and produces a `RenderOutput` with RGBA pixel data. **It currently only performs a clear pass with the background color — it does NOT render the image texture.** The image content is ignored entirely; only the background color is applied.

This is an explicit design choice for the skeleton stage. The full textured quad rendering is planned for Stage 010.

### Step-by-step pipeline

1. **Validate dimensions**: Returns `WgpuRenderError::InvalidViewport` if `width` or `height` is zero.

2. **Parse background color**: Converts the hex color string to `RgbaColor` using the same parser as the CPU renderer. Returns `WgpuRenderError::InvalidBackground` on parse failure.

3. **Create wgpu Instance**: Same as the probe — `Backends::all()`, no display required.

4. **Request Adapter**: With `force_fallback_adapter: true` and `compatible_surface: None`. Returns `WgpuRenderError::NoAdapter` if no adapter is available.

5. **Request Device and Queue**: Conservative limits (`downlevel_webgl2_defaults`), no special features. Returns `WgpuRenderError::DeviceCreation` on failure.

6. **Create output texture**: A 2D texture with format `Rgba8Unorm`, usage `RENDER_ATTACHMENT | COPY_SRC`. The texture dimensions match the viewport. `RENDER_ATTACHMENT` is needed for the clear pass; `COPY_SRC` is needed for reading back to CPU.

7. **Run clear pass**: A render pass with `LoadOp::Clear(bg_color)` and `StoreOp::Store`. The background color is converted from u8 [0..255] to float [0.0..1.0] for wgpu. No shader pipeline, no vertex buffer, no draw calls — just the clear operation.

8. **Copy texture to buffer**: Creates a `COPY_DST | MAP_READ` buffer. Calculates padded bytes-per-row alignment (wgpu requires `COPY_BYTES_PER_ROW_ALIGNMENT` = 256). Copies the texture to the buffer using `encoder.copy_texture_to_buffer()`.

9. **Submit and map**: Submits the command buffer, then maps the buffer for CPU read using `buffer_slice.map_async()` with a channel-based synchronization pattern. Polls the device with `PollType::Wait` to ensure the GPU work completes.

10. **Read pixels**: Iterates rows, stripping padding. Each row's unpadded bytes are copied into the output `Vec<u8>`. The result is contiguous RGBA8 pixel data matching the CPU renderer's output format.

11. **Return RenderOutput**: The same `RenderOutput` type used by the CPU renderer, enabling code that consumes render output to be backend-agnostic.

### What it does NOT do (yet)

- **No image texture upload**: The source image is not loaded, decoded, or uploaded to a GPU texture. This is the primary gap for Stage 010.
- **No shader pipeline**: No WGSL shaders, no render pipeline, no vertex/index buffers.
- **No textured quad**: No geometry is drawn. The render pass only clears.
- **No fit mode support**: Since no image is rendered, fit modes are irrelevant. The `background` color fills the entire viewport.
- **No alpha blending on GPU**: No composition of image over background.

## CLI Commands

### `wallflow wgpu-probe`

Probes the system for wgpu GPU capabilities and outputs a structured JSON report.

```bash
cargo run -p wallflow-cli -- wgpu-probe
```

Behavior:
- Calls `probe_wgpu_capabilities()`.
- Prints the full `WgpuRenderCapabilities` as pretty-printed JSON.
- If `supported: true`, prints human-readable adapter info.
- If `supported: false`, prints the failure reason.
- **Always exits with code 0** — this is a diagnostic, not a test.

Use cases:
- Pre-flight check before running GPU-dependent code.
- CI environment introspection (does the runner have a GPU?).
- Bug reports (users can share the probe output to help diagnose GPU issues).

### `wallflow wgpu-smoke`

Runs the wgpu offscreen render smoke test. If no GPU adapter is available, outputs a skipped report and exits with code 0.

```bash
cargo run -p wallflow-cli -- wgpu-smoke
```

Behavior:
1. Probes capabilities with `probe_wgpu_capabilities()`.
2. If `supported: false`, prints a skip report and exits 0. This is **expected** in CI.
3. If `supported: true`, runs `render_static_image_wgpu_offscreen(4, 4, "#ff0000")`.
4. Verifies the output:
   - Dimensions match (4×4).
   - First pixel is approximately red (R ≥ 250, G ≤ 5, B ≤ 5, A ≥ 250) — accounting for floating-point rounding in the u8→float→u8 round-trip.
5. Prints a structured JSON report including capabilities and render verification results.

Typical output on a GPU machine:
```json
{
  "test": "wgpu-smoke",
  "supported": true,
  "skipped": false,
  "success": true,
  "total_elapsed_ms": 42,
  "capabilities": { ... },
  "render_output": {
    "width": 4,
    "height": 4,
    "checksum": "abc123...",
    "dimensions_ok": true,
    "pixel_ok": true,
    "note": "clear-only experimental path (no image texture rendering)"
  }
}
```

Typical output in headless CI:
```json
{
  "test": "wgpu-smoke",
  "supported": false,
  "skipped": true,
  "failure_reason": "no adapter available: NotFound",
  "total_elapsed_ms": 3
}
```

## `--backend` Flag for `wallflow-renderer --headless-render-sim`

The `wallflow-renderer` binary accepts a `--backend` flag that selects the render backend:

```bash
# CPU reference renderer (default, cloud-testable)
cargo run -p wallflow-renderer -- \
  --headless-render-sim \
  --width 800 --height 450 \
  --source input.png \
  --render-output output.png \
  --backend cpu

# Experimental wgpu offscreen renderer (requires GPU)
cargo run -p wallflow-renderer -- \
  --headless-render-sim \
  --width 800 --height 450 \
  --render-output output.png \
  --backend wgpu
```

The flag accepts `"cpu"` (default) or `"wgpu"`. When `wgpu` is selected:

- `render_static_image_wgpu_offscreen()` is called instead of `render_static_image_cpu()`.
- The wgpu path ignores `--source` (no image texture upload yet). Only the background clear is performed.
- If no GPU adapter is available, the command returns an error with a clear message.
- The output PNG and JSON report follow the same format as the CPU path, ensuring downstream tooling is backend-agnostic.

Implementation in `render_static_image_for_sim()`:
```rust
let output = match backend {
    "wgpu" => {
        // Experimental wgpu offscreen render (clear-only, no image texture yet)
        let mut output = wallflow_render::render_static_image_wgpu_offscreen(
            viewport.width,
            viewport.height,
            "#000000",
        )?;
        output.save_png(output_path)?;
        output
    }
    _ => {
        // Default: CPU reference renderer
        let render_input = wallflow_render::StaticRenderInput { ... };
        let mut output = wallflow_render::render_static_image_cpu(render_input)?;
        output.save_png(output_path)?;
        output
    }
};
```

Note: The `--backend` flag is parsed as a raw string, not as the `RenderBackend` enum. This is because the renderer binary does not depend on the enum directly — it dispatches based on the string value. Future refactoring could use the `RenderBackend` enum for type safety.

## Why GPU Smoke MUST NOT Break CI

This is a critical design principle. The wgpu smoke tests must **never** cause CI to fail due to GPU unavailability. Here is why and how:

### Why

1. **No GPU in cloud CI**: Standard GitHub Actions runners, Azure DevOps agents, and similar cloud CI environments run on headless Linux VMs with no GPU. Vulkan, Metal, and Dx12 are typically unavailable. The software renderer (llvmpipe/Mesa) may or may not be installed.

2. **Platform heterogeneity**: Developers on macOS (Metal), Windows (Dx12/Vulkan), and Linux (Vulkan/OpenGL) have different GPU availability. Tests must not assume any particular backend.

3. **GPU driver instability**: Even when a GPU is present, drivers can crash, hang, or return unexpected errors. Tests that depend on GPU behavior are inherently less reliable than CPU tests.

4. **Separation of concerns**: CI validates the CPU reference path, the layout engine, the IPC protocol, the package format, and the renderer lifecycle. GPU rendering is an optional acceleration layer that must not gate the core pipeline.

### How

1. **`wgpu-probe` always exits 0**: The probe is a diagnostic tool. It reports capabilities but never fails. Even `supported: false` is a successful probe result.

2. **`wgpu-smoke` skips gracefully**: If no GPU adapter is found, the smoke test outputs a `"skipped": true` report and exits 0. CI sees a passing test, not a failure.

3. **`--backend cpu` is the default**: The `--backend` flag defaults to `cpu`. GPU code is only invoked when explicitly requested. CI never passes `--backend wgpu`.

4. **Unit tests don't require GPU**: All unit tests in `wallflow-render` (serialization, error display, `FromStr` for `RenderBackend`) are pure CPU tests. No test creates a wgpu instance.

5. **Error handling, not panics**: `WgpuRenderError::NoAdapter` is a recoverable error. The caller can fall back to the CPU renderer or present a user-friendly message. No `unwrap()`, no `panic!()` in the wgpu backend.

6. **Future: `#[ignore]` for GPU integration tests**: When full GPU rendering is implemented, integration tests that require a GPU should be marked `#[ignore]` and run separately with `cargo test -- --ignored` on machines with GPUs. They must not be part of the default `cargo test` run.

### Pattern for other GPU-dependent features

This pattern should be followed for any future GPU-dependent functionality:

```rust
// PATTERN: Graceful GPU degradation
match probe_wgpu_capabilities().supported {
    true => {
        // Use GPU path
        match gpu_operation() {
            Ok(result) => result,
            Err(WgpuRenderError::NoAdapter | WgpuRenderError::DeviceCreation(_)) => {
                // Fall back to CPU
                cpu_fallback()
            }
            Err(other) => return Err(other),
        }
    }
    false => cpu_fallback(),
}
```

## What Remains for Stage 010: Textured Quad Rendering

The wgpu backend skeleton establishes the infrastructure but stops short of actual image rendering. Stage 010 will fill these gaps:

### 1. Image Upload to GPU Texture

Currently, the source image is never loaded in the wgpu path. Stage 010 must:

- Decode the image using the `image` crate (same as the CPU renderer).
- Create a wgpu texture with the image dimensions and `Rgba8Unorm` format.
- Upload the decoded pixel data to the GPU texture using `queue.write_texture()`.
- Create a texture view and sampler for the shader pipeline.

Considerations:
- Large images (8K wallpapers = 7680×4320×4 = ~132 MB) may require staged uploads or tiling.
- The `max_texture_dimension_2d` limit must be checked before creating the texture.
- sRGB vs linear color space: `Rgba8UnormSrgb` vs `Rgba8Unorm` must be handled correctly for consistent color output.

### 2. Shader Pipeline

A WGSL shader program for rendering a textured quad:

- **Vertex shader**: Transforms a full-screen quad (two triangles) to clip space. Receives the destination rectangle from the layout engine as a uniform.
- **Fragment shader**: Samples the image texture at the correct UV coordinates, applies the fit mode transformation, and outputs the final color. The background color is used for areas not covered by the image.

This requires:
- A `wgpu::RenderPipeline` with vertex/fragment shader stages.
- A uniform buffer for the destination rectangle, viewport size, and background color.
- A bind group layout for the texture, sampler, and uniform buffer.
- A vertex buffer for the quad geometry (or procedurally generated in the shader).

### 3. Fit Mode Implementation in Shaders

Each fit mode must be implemented in the fragment shader:

| Fit Mode | UV Transformation | Background Visible |
|----------|-------------------|--------------------|
| Cover | Scale to fill, center, clip | No |
| Contain | Scale to fit, center | Yes (letterbox/pillarbox) |
| Stretch | Map UV to full viewport | No |
| Center | No scaling, centered | Yes (around image) |
| Tile | Repeat UV with fract() | No |

The shader must correctly handle aspect ratio, clipping, and background compositing. The CPU reference renderer serves as the specification.

### 4. Shader Uniform Layout

```wgsl
struct RenderUniforms {
    viewport_width: f32,
    viewport_height: f32,
    dest_x: f32,
    dest_y: f32,
    dest_width: f32,
    dest_height: f32,
    img_width: f32,
    img_height: f32,
    bg_r: f32,
    bg_g: f32,
    bg_b: f32,
    bg_a: f32,
    fit_mode: u32,  // 0=Cover, 1=Contain, 2=Stretch, 3=Center, 4=Tile
    opacity: f32,
}
```

### 5. Render Pipeline Assembly

The complete offscreen render flow for Stage 010:

1. Decode image → upload to GPU texture.
2. Calculate layout (reuse existing layout engine).
3. Build uniform buffer with layout parameters.
4. Create render pipeline (shader + texture + sampler + uniform).
5. Run render pass: clear with background, then draw textured quad.
6. Copy output texture → CPU buffer → `RenderOutput`.

### 6. CPU/GPU Output Consistency

A critical requirement for Stage 010: the GPU renderer must produce output that matches the CPU reference renderer within a defined tolerance. This requires:

- A comparison test that renders the same input with both backends.
- Pixel-level difference analysis (allowing for floating-point rounding in GPU shaders vs integer math in the CPU renderer).
- A maximum allowed delta (e.g., 1–2 color values per channel).

## REQUIRES_REAL_WINDOWS_VALIDATION

The following items cannot be fully validated without an interactive Windows session with a real display and GPU:

- **wgpu surface rendering on Windows Desktop Window Manager**: The offscreen render path works without a window, but presenting to a real Win32 surface behind desktop icons (WorkerW/Progman) requires a live DWM session.
- **GPU adapter selection on hybrid laptops**: Windows laptops with both integrated and discrete GPUs may present multiple adapters. The current code uses `force_fallback_adapter: true`, which may prefer the integrated GPU. The production renderer should use `PowerPreference::HighPerformance` for the desktop-attached window.
- **Dx12 backend behavior**: The wgpu Dx12 backend on Windows may have different error handling, texture format support, and performance characteristics compared to Vulkan on Linux. These must be tested on real Windows hardware.
- **Multi-monitor GPU context**: When the renderer is attached to a specific monitor, the GPU context must be created for the correct adapter/LUID. This requires multi-monitor hardware.
- **DPI scaling with GPU rendering**: Windows DPI scaling affects the swapchain size and the rendered viewport. The GPU renderer must produce the correct pixel dimensions for scaled displays.
- **GPU driver crash recovery**: If the GPU driver crashes (TDR on Windows), the renderer must detect the lost device and either recreate the GPU context or fall back to the CPU renderer. This can only be tested with real GPU driver behavior.
- **Fullscreen exclusive mode**: When a fullscreen application takes over the display, the wallpaper renderer's GPU context may be disrupted. Recovery behavior must be validated with real fullscreen apps.
- **Memory budget monitoring**: The `wgpu::MemoryBudgetThresholds` configuration for proactively reducing texture quality when GPU memory is low. This requires real GPU memory pressure scenarios.

These items are documented here as known gaps. They should be validated on Windows hardware before the wgpu backend is promoted from `WgpuExperimental` to a stable backend option.
