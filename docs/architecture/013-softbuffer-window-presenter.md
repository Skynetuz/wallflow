# 013 – Softbuffer Window Presenter

**Stage**: 010
**Status**: Complete
**Date**: 2026-05-31

## Overview

Stage 010 adds the first real presenter for static images: a `winit` window with CPU-rendered RGBA frames presented via `softbuffer`. The presenter takes the output of the CPU reference renderer, converts it to the softbuffer pixel format, and blits it to the window surface — all without GPU acceleration or Windows desktop attach.

The implementation introduces two new renderer modes (`--windowed-softbuffer` and `--presenter-sim`), the presenter type system (`PresenterBackend`, `PresenterState`, `SoftbufferPresenterConfig`, `PresenterReport`), pixel conversion functions (`rgba_to_softbuffer_u32`, `rgba_to_softbuffer_u32_with_surface_size`), and the `presenter-sim-smoke` CLI integration test. Total test count rose from 171 to 194.

## Why Softbuffer Before wgpu Textured Pipeline

The softbuffer presenter is implemented before a full wgpu textured rendering pipeline for several reasons:

1. **CPU reference renderer is already working** — the CPU renderer produces correct RGBA8 output for all five fit modes. Using this output directly via softbuffer gives us a working windowed presentation immediately, without needing to implement texture upload, shader pipeline, or GPU-side fit mode rendering.

2. **Separation of concerns** — rendering and presentation are independent concerns. The softbuffer presenter validates that the presentation layer works correctly with CPU-rendered output. When the wgpu textured pipeline is added later, it will produce the same RGBA output and can reuse the same presenter interface.

3. **Cloud-safe validation** — the `--presenter-sim` mode exercises the entire rendering + conversion pipeline without requiring a display server, making it suitable for CI and cloud testing. A wgpu textured pipeline would require GPU access that is unavailable in CI.

4. **Incremental progress** — each stage should deliver testable value. A working CPU→softbuffer presenter is more valuable than a partial wgpu pipeline that may not work in cloud environments.

## Presenter Architecture

### PresenterBackend

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PresenterBackend {
    #[default]
    SoftbufferCpu,
    WgpuExperimental,
}
```

The `PresenterBackend` enum identifies which presentation backend is in use. `SoftbufferCpu` is the default and only implemented backend — it uses the CPU reference renderer output and blits it to a softbuffer window surface. `WgpuExperimental` is reserved for a future wgpu-based presentation path that would use GPU texture upload and a swap chain instead of CPU blitting.

`PresenterBackend` implements `Display`, `FromStr` (accepting "softbuffer-cpu", "softbuffer", "cpu", "wgpu-experimental", "wgpu"), and `Serialize`/`Deserialize` for use in structured reports and CLI flags.

### PresenterState Lifecycle

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresenterState {
    Created,
    SurfaceReady,
    FrameRendered,
    Presented,
    Resized,
    Closed,
    Failed,
}
```

The presenter follows a state machine that mirrors the real window lifecycle:

```
Created → SurfaceReady → FrameRendered → Presented
              ↑              ↑              │
              └── Resized ───┘              │
                                             ↓
                                          Closed / Failed
```

- **Created**: The presenter has been constructed but no window or surface exists yet.
- **SurfaceReady**: The winit window has been created and the softbuffer context/surface initialized. This state is reached inside the `resumed()` callback of `SoftbufferPresenterApp`.
- **FrameRendered**: The CPU reference renderer has produced an RGBA frame. In `--windowed-softbuffer` mode, this happens during `RedrawRequested` events.
- **Presented**: The frame has been blitted to the softbuffer surface via `buffer_mut()` and `present()`.
- **Resized**: The window was resized; the viewport is updated and `needs_render` is set to trigger re-rendering.
- **Closed**: The window was closed or the event loop exited normally.
- **Failed**: An unrecoverable error occurred (e.g., softbuffer context creation failed, no display server).

### SoftbufferPresenterConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftbufferPresenterConfig {
    pub width: u32,
    pub height: u32,
    pub title: String,
    pub source: Option<PathBuf>,
    pub fit: String,
    pub background: String,
    pub timeout_secs: u64,
}
```

| Field | Type | Description |
|-------|------|-------------|
| width | u32 | Viewport width in pixels (must be > 0) |
| height | u32 | Viewport height in pixels (must be > 0) |
| title | String | Window title |
| source | Option<PathBuf> | Source image path (None = test pattern) |
| fit | String | Fit mode string (cover, contain, stretch, center, tile) |
| background | String | Background color hex (e.g. "#000000") |
| timeout_secs | u64 | Auto-exit timeout (0 = no timeout) |

The `validate()` method checks that width and height are non-zero and that the source file exists (if specified). Invalid configurations produce an error message string rather than panicking.

### PresenterReport

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenterReport {
    pub backend: PresenterBackend,
    pub viewport: PresenterViewport,
    pub source_path: Option<String>,
    pub rendered: bool,
    pub presented: bool,
    pub presented_simulated: bool,
    pub checksum: Option<String>,
    pub output_dimensions: Option<PresenterViewport>,
    pub error: Option<String>,
    pub exit_reason: Option<String>,
    pub duration_ms: Option<u64>,
}
```

The `PresenterReport` is the structured output of both `--presenter-sim` and `--windowed-softbuffer` modes. Key fields:

- **rendered**: Whether the CPU render completed successfully (image decode + pixel compositing).
- **presented**: Whether the frame was actually presented to a real window surface. Always `false` in `--presenter-sim` mode.
- **presented_simulated**: Whether the presentation was simulated (RGBA→softbuffer conversion succeeded without a real surface). Always `false` in `--windowed-softbuffer` mode.
- **checksum**: SHA-256 hex digest of the RGBA pixel data from the CPU renderer (same checksum as `RenderOutput::checksum_sha256()`).
- **exit_reason**: Why the presenter exited. Possible values: "simulated", "config-error", "conversion-error", "timeout", "close-requested", "error".
- **duration_ms**: Wall-clock time of the presenter run in milliseconds.

Example `--presenter-sim` output:

```json
{
  "backend": "SoftbufferCpu",
  "viewport": { "width": 800, "height": 450 },
  "source_path": "/tmp/wallflow-presenter-sim-smoke/test_image.png",
  "rendered": true,
  "presented": false,
  "presented_simulated": true,
  "checksum": "a1b2c3d4...",
  "output_dimensions": { "width": 800, "height": 450 },
  "error": null,
  "exit_reason": "simulated",
  "duration_ms": 42
}
```

## Pixel Conversion: RGBA8 → Softbuffer Format

### RGBA8 (CPU renderer output)

Each pixel is 4 bytes: R, G, B, A (red first, alpha last).

### Softbuffer u32 format

Each pixel is a `u32`: `0x00RRGGBB` — the highest 8 bits are zero, then red, green, blue in the lowest 24 bits. Alpha is not represented; the format is XRGB8888.

### Alpha handling

Since softbuffer does not support alpha blending against the desktop, the `rgba_to_softbuffer_u32()` function composites RGBA pixels against an opaque black background before discarding the alpha channel. Fully opaque pixels pass through unchanged; semi-transparent pixels are alpha-blended against black.

The compositing formula for each channel is: `final = round(component * alpha / 255)`. Two fast paths are used: `a == 255` passes through unchanged, `a == 0` becomes pure black.

### Endianness

The `u32` pixel value is constructed using native-endian bit operations: `(r as u32) << 16 | (g as u32) << 8 | b as u32`. On little-endian platforms (x86/x86-64), the in-memory byte order is B, G, R, 0x00. On big-endian, it is 0x00, R, G, B. This matches softbuffer's expectation because it interprets the `u32` as a native-endian integer.

### rgba_to_softbuffer_u32_with_surface_size

A companion function that handles size mismatches between the rendered frame and the target surface. When the rendered frame is smaller than the surface, the remaining pixels are filled with the specified background color. When larger, the frame is cropped. This is used after a window resize before the next re-render completes.

## Runtime Modes

### `--windowed-softbuffer`

Creates a real winit window, creates a softbuffer context and surface, and presents CPU-rendered frames to the window. The mode works as follows:

1. **Config validation**: `SoftbufferPresenterConfig::validate()` checks dimensions and source existence.
2. **Event loop creation**: `winit::event_loop::EventLoop::new()` is called. If no display server is available, this returns a clear error without panicking.
3. **Window creation**: In the `resumed()` callback, a `winit::Window` is created with the configured dimensions and title.
4. **Softbuffer setup**: Raw display/window handles are extracted from the winit window and wrapped in `DisplayHandleWrapper`/`WindowHandleWrapper`. A `softbuffer::Context` and `softbuffer::Surface` are created from these wrappers.
5. **Initial render**: `needs_render` is set to `true` after surface creation.
6. **Redraw loop**: On `RedrawRequested`, if `needs_render` is true, the CPU reference renderer produces an RGBA frame. The frame is converted via `rgba_to_softbuffer_u32()` and copied to the softbuffer buffer, then `present()` is called.
7. **Resize handling**: `WindowEvent::Resized` updates the viewport and sets `needs_render = true`, triggering re-render on the next redraw.
8. **Timeout**: `about_to_wait()` checks if the configured timeout has elapsed and calls `event_loop.exit()` if so.
9. **Close handling**: `WindowEvent::CloseRequested` and `WindowEvent::Destroyed` call `event_loop.exit()`.

If no `--source` is specified, a checkerboard test pattern is rendered instead of an image.

### `--presenter-sim`

Cloud-safe simulation mode. Performs the same CPU rendering and RGBA→softbuffer conversion, but does not create a window or surface. Outputs a structured JSON `PresenterReport` to stdout. Suitable for CI and cloud testing.

The simulation mode:

1. Validates the `SoftbufferPresenterConfig`.
2. Runs the CPU reference renderer (or generates a test pattern if no source).
3. Calls `rgba_to_softbuffer_u32()` to convert the RGBA output — this validates the conversion pipeline without requiring a surface.
4. Constructs a `PresenterReport` with `rendered: true`, `presented: false`, `presented_simulated: true`.
5. Prints the report as JSON to stdout. All diagnostic logs go to stderr.

If the config is invalid, the mode outputs a report with `rendered: false` and the error details, still exits with code 0 (the report itself indicates failure).

### Key Differences Between Modes

| Mode | Window | Surface | Output | Requires Display | Cloud-Safe |
|------|--------|---------|--------|------------------|------------|
| `--windowed-softbuffer` | Yes | softbuffer | Visual | Yes | No |
| `--presenter-sim` | No | None | JSON report | No | Yes |
| `--windowed-static` | Yes | None | Visual (blank) | Yes | No |
| `--headless-render-sim` | No | None | JSON report | No | Yes |
| `--ipc-stdio` | No | None | IPC frames | No | Yes |

## SoftbufferPresenterApp (ApplicationHandler)

The `--windowed-softbuffer` mode is implemented via a `SoftbufferPresenterApp` struct that implements winit 0.30's `ApplicationHandler` trait:

```rust
struct SoftbufferPresenterApp {
    renderer_id: RendererId,
    viewport: RendererViewport,
    source: Option<PathBuf>,
    fit: FitMode,
    fit_str: String,
    background: String,
    timeout: Option<Duration>,
    start: Instant,
    window: Option<winit::window::Window>,
    context: Option<softbuffer::Context<DisplayHandleWrapper>>,
    surface: Option<softbuffer::Surface<DisplayHandleWrapper, WindowHandleWrapper>>,
    needs_render: bool,
    last_render_output: Option<RenderOutput>,
}
```

Key design points:

- The `window`, `context`, and `surface` fields are `Option` because they are created inside `resumed()`, not at construction time.
- `needs_render` is a dirty flag that prevents re-rendering on every `RedrawRequested` when nothing has changed.
- `last_render_output` caches the most recent CPU render output, enabling re-presentation after resize without re-rendering.

## Softbuffer Context/Surface Creation

The softbuffer `Context` and `Surface` are created from raw window/display handles rather than from the `winit::Window` directly. This is necessary because:

1. `softbuffer::Context::new(display)` takes ownership of the display handle provider
2. `softbuffer::Surface::new(&context, window)` takes ownership of the window handle provider
3. `winit::Window` implements both `HasDisplayHandle` and `HasWindowHandle`, but is not `Clone`

The solution is to extract the raw `RawDisplayHandle` and `RawWindowHandle` from the window, wrap them in simple wrapper types that implement `HasDisplayHandle` and `HasWindowHandle`, and pass these to softbuffer. This is the same pattern winit uses internally.

```rust
struct DisplayHandleWrapper {
    raw: raw_window_handle::RawDisplayHandle,
}

impl raw_window_handle::HasDisplayHandle for DisplayHandleWrapper {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(unsafe { DisplayHandle::borrow_raw(self.raw) })
    }
}

struct WindowHandleWrapper {
    raw: raw_window_handle::RawWindowHandle,
}

impl raw_window_handle::HasWindowHandle for WindowHandleWrapper {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Ok(unsafe { WindowHandle::borrow_raw(self.raw) })
    }
}
```

The `unsafe` blocks are safe because the raw handles are valid for the lifetime of the wrapper structs, which are not moved or dropped while softbuffer holds references to them.

## Cloud Validation Strategy

The `presenter-sim-smoke` CLI command validates the full rendering + conversion pipeline in cloud environments:

1. Creates a temporary directory and generates a 2×2 test PNG fixture with distinct pixel colors.
2. Spawns the renderer in `--presenter-sim` mode with the test image as source.
3. Captures the JSON report from stdout (logs go to stderr, so there is no noise).
4. Parses the JSON into a `PresenterReport`-compatible structure.
5. Verifies: `rendered == true`, `presented_simulated == true`, `checksum` is present, `viewport` matches expected dimensions, no `error` field.

This smoke test runs in CI alongside the other cloud-safe smoke tests (ipc-supervisor-smoke, apply-static-smoke, render-sim-smoke, render-output-smoke, wgpu-probe).

## Behavior Without a Display Server

When `--windowed-softbuffer` is run in an environment without a display server (e.g., CI, SSH, container), the behavior is:

1. `winit::event_loop::EventLoop::new()` fails with an error.
2. The renderer catches this error, logs a warning, prints a clear error message to stderr, and returns `Err(anyhow!("failed to create winit event loop (no display server?): {e}"))`.
3. The process exits with a non-zero code.
4. **No panic occurs.** This is a strict design requirement: the renderer must never panic due to a missing display server.

This graceful degradation means `--windowed-softbuffer` can be used in diagnostic scripts that test whether a display server is available — the exit code and stderr message provide a clear answer.

## Test Coverage

Stage 010 adds 23 new tests to `wallflow-render`, bringing the total from 171 to 194:

### Pixel conversion tests (12)

| Test | Description |
|------|-------------|
| `test_rgba_to_softbuffer_red` | Red pixel → `0x00FF0000` |
| `test_rgba_to_softbuffer_green` | Green pixel → `0x0000FF00` |
| `test_rgba_to_softbuffer_blue` | Blue pixel → `0x000000FF` |
| `test_rgba_to_softbuffer_white` | White pixel → `0x00FFFFFF` |
| `test_rgba_to_softbuffer_black` | Black pixel → `0x00000000` |
| `test_rgba_to_softbuffer_transparent_is_black` | Fully transparent → black (composited) |
| `test_rgba_to_softbuffer_semi_transparent` | Semi-transparent alpha compositing against black |
| `test_rgba_to_softbuffer_invalid_length` | Buffer too short → error |
| `test_rgba_to_softbuffer_wrong_dimensions` | Width×height mismatch → error |
| `test_rgba_to_softbuffer_multi_pixel` | 2×1 frame with red + blue pixels |
| `test_rgba_to_softbuffer_padded_with_bg` | 1×1 render → 2×2 surface with background fill |
| `test_rgba_to_softbuffer_cropped` | 2×2 render → 1×1 surface with cropping |

### PresenterBackend tests (3)

| Test | Description |
|------|-------------|
| `test_presenter_backend_display` | Display trait formatting |
| `test_presenter_backend_from_str` | FromStr parsing with aliases |
| `test_presenter_backend_serde_roundtrip` | JSON serialization roundtrip |

### PresenterState tests (2)

| Test | Description |
|------|-------------|
| `test_presenter_state_display` | Display trait formatting |
| `test_presenter_state_serde_roundtrip` | JSON roundtrip for all 7 states |

### Config validation tests (4)

| Test | Description |
|------|-------------|
| `test_config_valid` | Valid config passes |
| `test_config_zero_width_rejected` | Width=0 rejected |
| `test_config_zero_height_rejected` | Height=0 rejected |
| `test_config_invalid_source_handled` | Missing source file detected |

### PresenterReport serialization tests (2)

| Test | Description |
|------|-------------|
| `test_presenter_report_serde_roundtrip` | Full report roundtrip |
| `test_presenter_report_error_case_serde` | Error report roundtrip |

## What Requires Real Windows Validation

- `--windowed-softbuffer` visual correctness on Windows (frame content, resize behavior)
- Frame content matches CPU reference renderer output on a real display
- Resize behavior (re-render, re-present, no flicker, no black flash)
- Desktop attachment behind Explorer icons
- Multi-monitor placement
- DPI scaling behavior
- Window style and chrome (popup vs overlapped)
- Explorer restart tolerance with winit event loop

## What Can Be Validated in Cloud

- CPU reference renderer output (dimensions, checksum, fit mode correctness)
- RGBA→softbuffer pixel conversion (all colors, alpha compositing, error handling)
- Presenter report structure and serialization
- Presenter configuration validation
- `--presenter-sim` mode (render + conversion + JSON output)
- `presenter-sim-smoke` integration test
- `--windowed-softbuffer` graceful error when no display server (no panic, clear message)
