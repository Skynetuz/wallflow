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

## MVP-2 static wallpaper

- Add wgpu rendering pipeline to the windowed static renderer.
- Per-monitor placement.
- Fullscreen detection pause policy.
- Connect desktop attach to winit window on Windows.

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
