# WallFlow roadmap

## MVP-0 Foundation

- Workspace structure.
- Shared domain model.
- Config load/save.
- IPC protocol contracts.
- Monitor diffing.
- Windows monitor enumeration first pass.
- Windows desktop attach first pass.
- Media backend abstraction.
- Renderer process smoke binary.
- CLI diagnostics.

## MVP-1 Windows proof

- Compile and test on Windows.
- Harden WorkerW/Progman discovery.
- Add dummy native window renderer.
- Attach renderer window behind desktop icons.
- Restart renderer after crash.

## MVP-1.5 Cloud-safe core integration ✅

*Completed in stage 003.*

- CoreApp with RendererSupervisor.
- Renderer lifecycle state machine (Starting → Running → Stale → Crashed → SafeMode).
- WatchdogPolicy with configurable heartbeat timeout, max restarts, restart window.
- RendererRestartPolicy (Never / Limited / Always).
- RendererHealth classification (Healthy / Stale / Unhealthy).
- RendererAssignment model (renderer ↔ monitor mapping).
- Typed IPC protocol v2: RendererCommand, RendererEvent, CoreCommand, CoreEvent.
- Headless heartbeat renderer mode (`--headless-heartbeat`).
- Supervisor smoke test (`supervisor-smoke` CLI command).
- 50 unit tests passing on Linux.
- GitHub Actions CI (Ubuntu + Windows).
- Cloud validation strategy documented.
- Windows cross-compilation tested: pure-logic crates compile; Win32 crates need Windows SDK libs.

### REQUIRES_REAL_WINDOWS_VALIDATION

- Desktop probe (Progman/WorkerW/SHELLDLL_DefView discovery).
- Desktop attach smoke test (embedding window behind icons).
- Explorer restart tolerance.
- Multi-monitor desktop attach.
- DPI change handling.

## MVP-1.7 Cloud-safe typed IPC renderer control ✅

*Completed in stage 004.*

- IPC protocol v3: `IpcMessage` tagged union for unambiguous frame decoding.
- `CommandEnvelope<T>` and `EventEnvelope<T>` with protocol version and request ID.
- Length-prefixed JSON framing: async (`read_frame`/`write_frame`) and sync
  (`encode_to_bytes`/`decode_from_bytes`) helpers.
- Frame validation: max size, invalid length, invalid JSON, protocol version mismatch.
- `--ipc-stdio` renderer mode: reads commands from stdin, writes events to stdout,
  all logs to stderr.
- Full IPC command/event lifecycle: Start, Pause, Resume, Stop, Shutdown,
  ApplyWallpaper, SetMonitor.
- `ipc-supervisor-smoke` CLI command: spawns renderer, exchanges typed IPC
  frames, validates complete lifecycle (Started → Ready → Heartbeat → Pause →
  Paused → Resume → Resumed → Shutdown → Exited).
- Legacy `--headless-heartbeat` mode preserved for backward compatibility.
- 67 unit tests passing on Linux (24 in wallflow-ipc, 25 in wallflow-core,
  11 in wallflow-desktop, 2 in wallflow-config, 2 in wallflow-media, 3 in
  wallflow-monitor).
- GitHub Actions CI updated with IPC smoke test.
- IPC contract documented in `docs/architecture/002-ipc-contract.md`.

## MVP-1.9 Wallpaper package and apply contract ✅

*Completed in stage 005.*

- Wallpaper package format v0 with manifest.json.
- `wallflow-package` crate: load, parse, validate wallpaper packages.
- Static image wallpaper model: image path, fit mode, background color, opacity.
- Package validation: schema version, required fields, kind support, path traversal prevention, asset existence.
- IPC protocol v4: `ApplyWallpaperRequest`, `StaticImagePayload`, `WallpaperPayload`, `WallpaperApplyError`, `FitMode`.
- Renderer `AppliedWallpaperState`: validates and records applied wallpaper via IPC.
- `apply-static-smoke` CLI command: full package → validate → apply → confirm lifecycle.
- 97 unit tests passing on Linux.
- IPC supervisor smoke and apply-static-smoke both passing.
- Package validation documented in `docs/architecture/008-wallpaper-package-format.md`.

### REQUIRES_REAL_WINDOWS_VALIDATION

- Desktop probe (Progman/WorkerW/SHELLDLL_DefView discovery).
- Desktop attach smoke test (embedding window behind icons).
- Explorer restart tolerance.
- Multi-monitor desktop attach.
- DPI change handling.

## MVP-2.0 Cloud-safe static image decode and layout ✅

*Completed in stage 006.*

- `FitMode` unified in `wallflow_common`: single canonical definition, re-exported by
  `wallflow-package` and `wallflow-ipc`.
- `ImageMetadata` type in `wallflow-package`: width, height, color_type, detected_format,
  file_size_bytes.
- `load_image_metadata()` function: reads image dimensions and format without full
  pixel decode, using `image::io::Reader`.
- `ImageDecodeError` type for image decode failures.
- `validate_package_deep()`: structural + image decode validation.
- Layout engine in `wallflow-package::layout`:
  - `Viewport`, `ImageSize`, `RenderRect`, `StaticImageLayout` types.
  - `calculate_static_image_layout()` with cover, contain, stretch, center, tile fit modes.
  - `LayoutError` for zero dimension rejection.
- IPC protocol v5: `AppliedWallpaperReport`, `StaticImageApplyReport`,
  `IpcImageMetadata`, `StaticImageLayoutReport`.
- `WallpaperApplied` event now includes `report: Option<AppliedWallpaperReport>`.
- Renderer decodes image metadata and calculates layout on `ApplyWallpaper`.
- `LoadedStaticImageState` replaces `AppliedWallpaperState`.
- `apply-static-smoke` uses real 2×2 PNG (via `image` crate), deep validation,
  and verifies image dimensions, layout rect, and wallpaper_id in the report.
- `package-validate` CLI command with optional `--deep` flag.
- `image` crate added as workspace dependency.
- Rendering model documented in `docs/architecture/009-static-image-rendering-model.md`.

### REQUIRES_REAL_WINDOWS_VALIDATION

- Desktop probe (Progman/WorkerW/SHELLDLL_DefView discovery).
- Desktop attach smoke test (embedding window behind icons).
- Explorer restart tolerance.
- Multi-monitor desktop attach.
- DPI change handling.
- Layout with actual monitor dimensions (not synthetic 1920×1080).
- Visual correctness of each fit mode.

## MVP-2.1 Windowed static renderer ✅

*Completed in stage 007.*

- `winit` 0.30 added as workspace dependency for cross-platform window management.
- `RendererRuntimeMode` enum: `HeadlessIpc`, `HeadlessRenderSim`, `WindowedStatic`.
- `RendererRuntimeState` lifecycle: Starting → Ready → Running → Paused → ShuttingDown → Exited / Failed.
- `RendererViewport` type with width, height, and optional DPI scale factor.
- `WindowRuntimeConfig` type for windowed renderer parameters.
- `RenderSimReport` and `RenderSimLayoutReport` structured output types.
- `--headless-render-sim` renderer mode: synthetic viewport, no display server, JSON report.
- `--windowed-static` renderer mode: winit 0.30 ApplicationHandler event loop, real window.
- `render-sim-smoke` CLI command: full renderer lifecycle + wallpaper apply + layout verification in CI.
- Viewport resize handling with automatic layout recalculation in `--windowed-static` mode.
- `ApplicationHandler` pattern for winit 0.30 (resumed creates window, about_to_wait checks timeout).
- Desktop attach not yet connected to winit window (requires REQUIRES_REAL_WINDOWS_VALIDATION).
- Renderer runtime architecture documented in `docs/architecture/010-renderer-window-runtime.md`.

### REQUIRES_REAL_WINDOWS_VALIDATION

- `--windowed-static` on Windows (winit creates a Win32 window; must verify on real desktop)
- Desktop attach with winit window (`SetParent()` on winit-managed HWND is untested)
- Viewport resize from real monitor events (not just synthetic resize)
- Layout with actual monitor dimensions (not synthetic 1920×1080)
- Visual correctness of each fit mode
- DPI scale factor changes (WM_DPICHANGED)
- Explorer restart tolerance with winit event loop

## MVP-2.2 Cloud-safe static render output ✅

*Completed in stage 008.*

- `wallflow-render` crate: CPU reference renderer producing actual RGBA pixel data.
- `RenderBackend` enum: `CpuReference` (and `WgpuExperimental` placeholder).
- `RenderOutput` type: width, height, pixels_rgba, optional output_path.
- `RenderOutputMetadata` type: dimensions, pixel format, file size, SHA-256 checksum.
- `StaticRenderInput` type: image_path, viewport, fit, background, opacity.
- `StaticRenderError` error type: invalid image path, decode, background, viewport, layout, I/O.
- `render_static_image_cpu()` function: full CPU render pipeline (decode → layout → composite → output).
- `RgbaColor` type with `parse_hex()`: safe `#RRGGBB` and `#RRGGBBAA` parsing.
- SHA-256 checksum for render output (via `sha2` crate) for snapshot-like tests.
- All five fit modes rendered with correct pixel output:
  - **Cover**: scale to fill viewport, center, clip overflow.
  - **Contain**: scale to fit within viewport, center, background bars.
  - **Stretch**: distort to exact viewport, no background.
  - **Center**: no scaling, centered, background visible.
  - **Tile**: repeat at native size, fill viewport.
- Nearest-neighbor scaling (temporary; bilinear/Lanczos later).
- `--headless-render-sim` enhanced with `--source`, `--render-output`, and `--fit` flags.
- `render-output-smoke` CLI command: creates test PNG, runs full render, verifies output.
- 164 tests passing (25 in wallflow-render including pixel-level tests).
- `render-output-smoke` added to CI on Ubuntu.
- Render output architecture documented in `docs/architecture/011-static-render-output.md`.

### REQUIRES_REAL_WINDOWS_VALIDATION

- `--windowed-static` on Windows (winit creates a Win32 window; must verify on real desktop)
- Desktop attach with winit window (`SetParent()` on winit-managed HWND is untested)
- Viewport resize from real monitor events (not just synthetic resize)
- Layout with actual monitor dimensions
- Visual correctness of each fit mode on a real display
- DPI scale factor changes (WM_DPICHANGED)
- Explorer restart tolerance with winit event loop
- wgpu GPU rendering matching CPU reference output

- Add wgpu rendering pipeline to the windowed static renderer.
- Per-monitor placement.
- Fullscreen detection pause policy.
- Connect desktop attach to winit window on Windows.

## MVP-2.3 Cloud-safe wgpu static backend skeleton ✅

*Completed in stage 009.*

- `wgpu` 29 added as workspace dependency for cross-platform GPU access.
- `RenderBackend` enum: `CpuReference` (default, cloud-testable) and `WgpuExperimental` (optional).
- `WgpuRenderCapabilities` type: adapter name, backend, device type, features, limits, supported flag, failure reason.
- `WgpuRenderError` type: NoAdapter, DeviceCreation, RenderFailed, BufferMap, FeatureNotSupported, InvalidViewport, InvalidBackground, NotImplemented.
- `probe_wgpu_capabilities()`: detects GPU adapter, creates device, reports capabilities. Never panics — returns structured error when GPU unavailable.
- `render_static_image_wgpu_offscreen()`: **clear-only experimental** offscreen render path. Creates texture, runs clear pass with background color, copies to CPU buffer. Does NOT render image texture yet (Stage 010).
- `wgpu-probe` CLI command: runs capability probe, outputs JSON, always exit 0.
- `wgpu-smoke` CLI command: if no GPU, outputs `{ "supported": false, "skipped": true }` and exit 0; if GPU available, runs minimal offscreen render and verifies output.
- `--backend cpu|wgpu` flag added to `wallflow-renderer --headless-render-sim`. Default: `cpu`.
- 171 tests passing (7 new wgpu-related tests in wallflow-render).
- wgpu-probe and wgpu-smoke work correctly. GPU smoke skips gracefully in headless CI.
- CPU reference renderer remains stable and unchanged.
- wgpu backend architecture documented in `docs/architecture/012-wgpu-static-backend.md`.

### REQUIRES_REAL_WINDOWS_VALIDATION

- `--windowed-static` on Windows with real GPU
- Desktop attach with winit window
- wgpu GPU rendering matching CPU reference output
- DPI scale factor handling
- Explorer restart tolerance with winit event loop
- Real GPU adapter capabilities on Windows hardware

## MVP-2.4 Cloud-safe softbuffer window presenter ✅

*Completed in stage 010.*

- `softbuffer` 0.4 added as workspace dependency for CPU-to-window pixel blitting.
- `raw-window-handle` 0.6 added for raw handle extraction from winit Window.
- `PresenterBackend` enum: `SoftbufferCpu` (default), `WgpuExperimental` (reserved).
- `PresenterState` lifecycle: Created → SurfaceReady → FrameRendered → Presented → Closed/Failed.
- `SoftbufferPresenterConfig` with validation (zero dimensions, missing source).
- `PresenterReport` structured output: backend, viewport, rendered, presented, presented_simulated, checksum, error, exit_reason, duration_ms.
- `rgba_to_softbuffer_u32()` pixel conversion: RGBA8 → softbuffer `0x00RRGGBB` format with alpha compositing against black background.
- `rgba_to_softbuffer_u32_with_surface_size()` for size-mismatch handling (pad/crop).
- `--windowed-softbuffer` renderer mode: winit window + softbuffer context/surface + CPU render → blit → present.
- `--presenter-sim` renderer mode: cloud-safe simulation, no window, JSON report to stdout.
- `presenter-sim-smoke` CLI command: creates test PNG, runs presenter-sim, verifies JSON report.
- Raw window/display handle wrappers for softbuffer (winit Window is not Clone).
- Resize handling: viewport update + re-render + re-present.
- Timeout and close-request: clean event loop exit.
- Logs to stderr in presenter-sim mode; stdout reserved for structured output.
- Graceful error when no display server (no panic, clear error message).
- 194 tests passing (23 new presenter-related tests in wallflow-render).
- `presenter-sim-smoke` added to CI.
- Softbuffer presenter architecture documented in `docs/architecture/013-softbuffer-window-presenter.md`.

### REQUIRES_REAL_WINDOWS_VALIDATION

- `--windowed-softbuffer` visual correctness on Windows
- Frame content matches CPU reference renderer output
- Resize behavior (re-render, re-present, no flicker)
- Desktop attach with winit window
- DPI scale factor handling
- Explorer restart tolerance with winit event loop
- Real GPU adapter capabilities on Windows hardware
- wgpu GPU rendering matching CPU reference output

## MVP-2.5 Windows desktop attach integration (planned: Stage 011)

- Connect `wallflow-desktop` desktop attach to the winit window from `--windowed-softbuffer`.
- Extract HWND from winit window via `raw_window_handle()` and call `attach_window_to_desktop()`.
- Handle Explorer restart by monitoring WorkerW handle validity in the event loop.
- Detach on shutdown via `detach_window_from_desktop()`.
- Requires REQUIRES_REAL_WINDOWS_VALIDATION — cannot be tested in CI.
- Alternative: wgpu textured rendering pipeline (GPU image compositing, replacing softbuffer for production).

## MVP-3 video wallpaper

- Implement Media Foundation backend.
- Hardware decode where available.
- Muted looping video.
- Fallback on Windows N missing media features.

## MVP-4 UI

- Tauri 2 + React UI.
- Library list.
- Monitor list.
- Apply wallpaper.
- Diagnostics panel.

## v1

- Package format.
- Web wallpapers as isolated renderer.
- App rules.
- Playlists.
- Hotkeys.
- Updater.

## v2

- Linux X11 first.
- Selective Wayland support.
- Online catalog.
- Audio-reactive API.
- Plugin sandboxing.
