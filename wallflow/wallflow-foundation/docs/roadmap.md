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

## MVP-2 static wallpaper

- Add winit/wgpu static renderer.
- Per-monitor placement.
- Fullscreen detection pause policy.
- Connect IPC pipe to renderer process.

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
