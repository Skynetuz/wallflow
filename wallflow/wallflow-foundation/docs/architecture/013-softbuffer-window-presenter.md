# 013 – Softbuffer Window Presenter

**Stage**: 010  
**Status**: Complete  
**Date**: 2026-05-30

## Overview

Stage 010 adds the first real presenter for static images: a `winit` window with CPU-rendered RGBA frames presented via `softbuffer`. The presenter takes the output of the CPU reference renderer, converts it to the softbuffer pixel format, and blits it to the window surface — all without GPU acceleration or Windows desktop attach.

## Why Softbuffer Before wgpu Textured Pipeline

The softbuffer presenter is implemented before a full wgpu textured rendering pipeline for several reasons:

1. **CPU reference renderer is already working** — the CPU renderer produces correct RGBA8 output for all five fit modes. Using this output directly via softbuffer gives us a working windowed presentation immediately, without needing to implement texture upload, shader pipeline, or GPU-side fit mode rendering.

2. **Separation of concerns** — rendering and presentation are independent concerns. The softbuffer presenter validates that the presentation layer works correctly with CPU-rendered output. When the wgpu textured pipeline is added later, it will produce the same RGBA output and can reuse the same presenter interface.

3. **Cloud-safe validation** — the `--presenter-sim` mode exercises the entire rendering + conversion pipeline without requiring a display server, making it suitable for CI and cloud testing. A wgpu textured pipeline would require GPU access that is unavailable in CI.

4. **Incremental progress** — each stage should deliver testable value. A working CPU→softbuffer presenter is more valuable than a partial wgpu pipeline that may not work in cloud environments.

## Presenter Architecture

### PresenterBackend

```
PresenterBackend
├── SoftbufferCpu    — CPU-rendered frames, softbuffer blit to window
└── WgpuExperimental — reserved for future wgpu-based presentation
```

### PresenterState Lifecycle

```
Created → SurfaceReady → FrameRendered → Presented
              ↑              ↑              │
              └── Resized ───┘              │
                                             ↓
                                          Closed / Failed
```

### SoftbufferPresenterConfig

| Field | Type | Description |
|-------|------|-------------|
| width | u32 | Viewport width in pixels |
| height | u32 | Viewport height in pixels |
| title | String | Window title |
| source | Option<PathBuf> | Source image path (None = test pattern) |
| fit | String | Fit mode string (cover, contain, stretch, center, tile) |
| background | String | Background color hex |
| timeout_secs | u64 | Auto-exit timeout (0 = no timeout) |

## Pixel Conversion: RGBA8 → Softbuffer Format

### RGBA8 (CPU renderer output)

Each pixel is 4 bytes: R, G, B, A (red first, alpha last).

### Softbuffer u32 format

Each pixel is a `u32`: `0x00RRGGBB` — the highest 8 bits are zero, then red, green, blue in the lowest 24 bits. Alpha is not represented; the format is XRGB8888.

### Alpha handling

Since softbuffer does not support alpha blending against the desktop, the `rgba_to_softbuffer_u32()` function composites RGBA pixels against an opaque black background before discarding the alpha channel. Fully opaque pixels pass through unchanged; semi-transparent pixels are alpha-blended against black.

### Endianness

The `u32` pixel value is constructed using native-endian bit operations: `(r as u32) << 16 | (g as u32) << 8 | b as u32`. On little-endian platforms (x86/x86-64), the in-memory byte order is B, G, R, 0x00. On big-endian, it is 0x00, R, G, B. This matches softbuffer's expectation because it interprets the `u32` as a native-endian integer.

## Runtime Modes

### `--windowed-softbuffer`

Creates a real winit window, creates a softbuffer context and surface, and presents CPU-rendered frames to the window. Requires a display server (Wayland/X11 on Linux, Desktop on Windows). Not suitable for CI/cloud environments.

### `--presenter-sim`

Cloud-safe simulation mode. Performs the same CPU rendering and RGBA→softbuffer conversion, but does not create a window or surface. Outputs a structured JSON `PresenterReport` to stdout. Suitable for CI and cloud testing.

### Key Differences Between Modes

| Mode | Window | Surface | Output | Requires Display | Cloud-Safe |
|------|--------|---------|--------|------------------|------------|
| `--windowed-softbuffer` | Yes | softbuffer | Visual | Yes | No |
| `--presenter-sim` | No | None | JSON report | No | Yes |
| `--windowed-static` | Yes | None | Visual (blank) | Yes | No |
| `--headless-render-sim` | No | None | JSON report | No | Yes |
| `--ipc-stdio` | No | None | IPC frames | No | Yes |

## Softbuffer Context/Surface Creation

The softbuffer `Context` and `Surface` are created from raw window/display handles rather than from the `winit::Window` directly. This is necessary because:

1. `softbuffer::Context::new(display)` takes ownership of the display handle provider
2. `softbuffer::Surface::new(&context, window)` takes ownership of the window handle provider
3. `winit::Window` implements both `HasDisplayHandle` and `HasWindowHandle`, but is not `Clone`

The solution is to extract the raw `RawDisplayHandle` and `RawWindowHandle` from the window, wrap them in simple wrapper types that implement `HasDisplayHandle` and `HasWindowHandle`, and pass these to softbuffer. This is the same pattern winit uses internally.

## Cloud Validation Strategy

The `presenter-sim-smoke` CLI command validates the full rendering + conversion pipeline in cloud environments:

1. Creates a test PNG fixture
2. Spawns the renderer in `--presenter-sim` mode
3. Parses the JSON report
4. Verifies: rendered=true, presented_simulated=true, checksum present, viewport correct, no errors

This smoke test runs in CI alongside the other cloud-safe smoke tests (ipc-supervisor-smoke, apply-static-smoke, render-sim-smoke, render-output-smoke, wgpu-probe).

## What Requires Real Windows Validation

- `--windowed-softbuffer` visual correctness on Windows (frame content, resize behavior)
- Desktop attachment behind Explorer icons
- Multi-monitor placement
- DPI scaling behavior
- Window style and Chrome (popup vs overlapped)

## What Can Be Validated in Cloud

- CPU reference renderer output (dimensions, checksum, fit mode correctness)
- RGBA→softbuffer pixel conversion (all colors, alpha compositing, error handling)
- Presenter report structure and serialization
- Presenter configuration validation
- `--presenter-sim` mode (render + conversion + JSON output)
- `presenter-sim-smoke` integration test
