# 010 – Renderer Window Runtime

> Stage: `007-windowed-static-renderer`
> Status: Implemented
> Date: 2026-06-01

## Why winit Was Added Before wgpu

WallFlow's renderer architecture introduces window management before the GPU
rendering pipeline, and this ordering is deliberate. In a wallpaper engine the
window lifecycle is the foundation on which everything else is built: the
renderer process must be able to open, position, resize, and close a window
before it can ever draw a single pixel with wgpu. By establishing the window
runtime independently, we achieve several goals:

1. **Decoupled validation.** The window lifecycle (creation, resize, destroy)
   can be tested and stabilized without any GPU dependencies. This is
   especially important for a project that develops primarily on Linux but
   targets Windows; the window system's behavior varies significantly between
   platforms, and we want to lock down the winit integration before adding the
   complexity of a wgpu surface and swap chain.

2. **Incremental complexity.** Adding wgpu requires handling adapter selection,
   surface creation, swap chain format negotiation, and shader pipeline setup.
   Each of these is a source of platform-specific bugs. By deferring wgpu, we
   can ship a windowed renderer that validates viewport tracking, layout
   recalculation, and the ApplicationHandler event loop without GPU concerns.

3. **Cloud-testable headless path.** The `--headless-render-sim` mode exercises
   the same runtime types and state machine as the windowed renderer without
   requiring a display server. This would not be possible if the runtime were
   tightly coupled to wgpu surface creation from the start.

4. **Desktop attach integration.** On Windows, the wallpaper window must be
   parented into the WorkerW hierarchy behind desktop icons. This reparenting
   operation is pure Win32 window management — it has nothing to do with GPU
   rendering. By shipping window management first, we lay the groundwork for
   the future step where the winit window is combined with desktop attach.

## Three Renderer Runtime Modes

The renderer process supports three mutually exclusive runtime modes, selected
via command-line flags. Each mode exercises a different layer of the system.

### `--ipc-stdio` (HeadlessIpc)

The headless IPC mode is the primary communication channel between the CoreApp
supervisor and the renderer process. It runs without any window or display
server dependency, making it fully cloud-testable. The renderer reads typed
`RendererCommand` frames from stdin and writes `RendererEvent` frames to
stdout, with all diagnostic logs directed to stderr. This mode supports the
full command lifecycle: Start, Pause, Resume, Stop, Shutdown, ApplyWallpaper,
and SetMonitor.

Use this mode when:
- Running under the CoreApp supervisor in production
- Running the `ipc-supervisor-smoke` or `apply-static-smoke` integration tests
- Any environment where a display server is unavailable (CI, SSH, containers)

### `--headless-render-sim` (HeadlessRenderSim)

The headless render simulation mode exercises the full renderer lifecycle —
including image decode, layout calculation, and state machine transitions —
without requiring a display server or window. It operates against a synthetic
viewport of configurable dimensions and produces a structured `RenderSimReport`
as JSON on stdout. This report includes every state transition, the viewport
used, whether a wallpaper was applied, and the complete layout calculation
result.

Use this mode when:
- Validating the renderer's internal logic in CI without a display server
- Running the `render-sim-smoke` CLI integration test
- Debugging layout calculations for specific viewport sizes
- Testing wallpaper apply failures (invalid image, zero dimensions, etc.)

### `--windowed-static` (WindowedStatic)

The windowed static mode opens a real winit window and displays a static
wallpaper image. This is the closest mode to production: it uses the winit 0.30
ApplicationHandler event loop, tracks viewport resizes, and recalculates layout
when the window dimensions change. It requires a display server (Wayland/X11 on
Linux, the Windows desktop on Windows).

Use this mode when:
- Testing the renderer with a real window on a development machine
- Validating viewport resize handling and layout recalculation
- Preparing for the future wgpu rendering pipeline (the window is already open)
- Manual visual verification of wallpaper display

Note: The current `--windowed-static` mode does not yet render pixels to the
window surface. It opens the window, tracks viewport state, and recalculates
layout on resize, but the actual GPU blit will be added when wgpu is
integrated in a future stage.

## Core Types

### RendererRuntimeMode

```rust
pub enum RendererRuntimeMode {
    HeadlessIpc,
    HeadlessRenderSim,
    WindowedStatic,
}
```

A `Copy` enum that identifies which runtime mode the renderer process is
running in. The CLI flags (`--ipc-stdio`, `--headless-render-sim`,
`--windowed-static`) map 1:1 to these variants. The mode is included in the
`RenderSimReport` so that test consumers can verify the correct mode was used.

### RendererRuntimeState

```rust
pub enum RendererRuntimeState {
    Starting,
    Ready,
    Running,
    Paused,
    ShuttingDown,
    Exited,
    Failed,
}
```

A `Copy` enum representing the lifecycle state of the renderer runtime. This
is distinct from `RendererState` (which tracks the supervisor's view of the
renderer process) — `RendererRuntimeState` tracks the runtime's internal
state as seen from inside the renderer process itself. Key methods:

- `is_terminal()` — Returns `true` for `Exited` and `Failed`, indicating the
  runtime will not transition further.
- `is_alive()` — Returns `true` for `Ready`, `Running`, and `Paused`,
  indicating the runtime is operational.

In `--headless-render-sim` mode, every state transition is recorded in the
`state_transitions` field of the `RenderSimReport`, giving full visibility into
the lifecycle that occurred during the simulation.

### RendererViewport

```rust
pub struct RendererViewport {
    pub width: u32,
    pub height: u32,
    pub scale_factor: Option<u32>,
}
```

Tracks the current viewport dimensions in logical pixels, with an optional
DPI scale factor. The viewport is the renderer's understanding of the display
area it targets. In `--headless-render-sim` mode, the viewport is initialized
from the `--width` and `--height` CLI arguments. In `--windowed-static` mode,
the viewport is updated on every `WindowEvent::Resized` event from winit.

The `is_valid()` method returns `false` if either dimension is zero — this
guards against degenerate viewports that would cause division-by-zero errors
in layout calculation. When an invalid viewport is detected in render-sim
mode, the runtime transitions to `Failed` and produces a report with
`exit_code: 1`.

### WindowRuntimeConfig

```rust
pub struct WindowRuntimeConfig {
    pub width: u32,
    pub height: u32,
    pub title: String,
    pub visible: bool,
    pub borderless: bool,
}
```

Configuration for the windowed renderer runtime. This type captures the
parameters needed to create a winit window: dimensions, title, visibility, and
whether the window should be borderless (no title bar or decorations). The
`Default` implementation creates a 1920×1080 visible titled window.

In the current implementation, `WindowRuntimeConfig` is constructed from the
CLI arguments and used only in `--windowed-static` mode. In the future, when
the renderer is driven by IPC commands, the config may be supplied by the
CoreApp supervisor via an initialization frame.

## Desktop Attach: Not Connected Yet

The Windows desktop attach functionality (`wallflow-desktop` crate) is
implemented and tested in isolation (see `docs/architecture/004-windows-desktop-attach.md`),
but it is not yet connected to the winit-based `--windowed-static` mode. There
are several reasons for this:

1. **winit owns the window.** The `--windowed-static` mode creates its window
   through winit's `event_loop.create_window()` API. winit 0.30 manages the
   window lifecycle internally, and calling `SetParent()` on a winit-managed
   window requires careful coordination — winit may recreate the window
   internally on platform events, which would break the reparenting.

2. **Event loop incompatibility.** The desktop attach mode uses a Win32
   `PeekMessageW` loop, while the windowed static mode uses winit's
   `EventLoop::run_app()`. These are two different event loop paradigms. To
   combine them, the winit window must be created first, then its underlying
   HWND must be extracted and reparented into the WorkerW hierarchy, all while
   winit's event loop continues to run.

3. **Requires REQUIRES_REAL_WINDOWS_VALIDATION.** The combination of winit
   window management and Win32 desktop attach has never been tested on a real
   Windows desktop. The interaction between winit's internal window state and
   the `SetParent()` call is unknown and may cause crashes, visual glitches,
   or event loop deadlocks.

The recommended approach for connecting desktop attach is:

1. After `resumed()` creates the winit window, extract the raw HWND using
   `window.raw_window_handle()` (via the `raw-window-handle` crate).
2. Call `attach_window_to_desktop()` from `wallflow-desktop`.
3. Monitor the WorkerW handle validity in the event loop and re-attach if
   Explorer restarts.
4. On `WindowEvent::Destroyed` or shutdown, call `detach_window_from_desktop()`
   before allowing winit to destroy the window.

This work is planned for a future stage that specifically targets Windows
desktop integration with winit.

## Cloud-Testable vs. Windows-Only

### What Can Be Tested in Linux/CI (No Display Server)

The following components and modes are fully testable on Linux and in cloud CI
environments without any display server:

| Component | Mode | Test Command |
|-----------|------|-------------|
| Runtime type definitions | All | `cargo test -p wallflow-common` |
| RendererRuntimeMode Display | All | Unit test |
| RendererRuntimeState transitions | All | Unit test |
| RendererViewport validity | All | Unit test |
| WindowRuntimeConfig defaults | All | Unit test |
| RenderSimReport serialization | All | Unit test |
| Headless render simulation | `--headless-render-sim` | `cargo run -p wallflow-renderer -- --headless-render-sim --width 800 --height 450 --timeout-secs 2` |
| Render sim smoke test | `--headless-render-sim` | `cargo run -p wallflow-cli -- render-sim-smoke --timeout-secs 5 --width 800 --height 450` |
| IPC stdio mode | `--ipc-stdio` | `cargo run -p wallflow-cli -- ipc-supervisor-smoke` |
| Apply static smoke | `--ipc-stdio` | `cargo run -p wallflow-cli -- apply-static-smoke` |
| Layout calculation | All | `cargo test -p wallflow-package` |

The `render-sim-smoke` CLI command is the primary integration test for the
renderer runtime. It creates a test wallpaper package, spawns the renderer in
`--headless-render-sim` mode, captures the structured JSON report from stdout,
and verifies:
- The report has `exit_code: 0`
- The `state_transitions` include Starting → Ready → Running → ShuttingDown → Exited
- The `wallpaper_applied` flag is `true`
- The `layout_report` contains valid image and viewport dimensions
- The destination rectangle is non-zero

### REQUIRES_REAL_WINDOWS_VALIDATION

The following items cannot be validated without a real Windows desktop session:

| Item | Reason |
|------|--------|
| `--windowed-static` on Windows | Requires a Windows desktop session; winit creates a Win32 window |
| Desktop attach with winit window | `SetParent()` on a winit-managed HWND is untested |
| Viewport resize from real monitor events | Requires actual display hardware or VM display |
| Layout with actual monitor dimensions | Currently uses synthetic 1920×1080; real monitors may differ |
| Visual correctness of fit modes | Cover cropping, contain letterboxing, tile repeat — must be seen |
| DPI scale factor changes (WM_DPICHANGED) | Requires Windows HiDPI environment |
| Explorer restart tolerance | Must kill and restart Explorer to test |

All code paths that depend on a real Windows desktop are marked with the
`REQUIRES_REAL_WINDOWS_VALIDATION` comment convention.

## ApplicationHandler Pattern (winit 0.30)

The `--windowed-static` mode uses winit 0.30's `ApplicationHandler` trait,
which replaced the older `EventHandler` trait. The `ApplicationHandler` pattern
is a significant API change that affects how the renderer event loop is
structured.

### Key Differences from EventHandler

In winit 0.30, the application implements `ApplicationHandler` instead of
`EventHandler`. The main differences are:

1. **Window creation is deferred.** The `ApplicationHandler` does not receive
   a window in its constructor. Instead, the window is created inside the
   `resumed()` callback, which is called when the event loop is ready. This
   allows the event loop to handle platform-specific initialization before any
   window is created.

2. **Per-window events.** The `window_event()` callback receives a `WindowId`
   parameter, allowing a single `ApplicationHandler` to manage multiple windows
   (important for future multi-monitor support).

3. **`about_to_wait` replaces `MainEventsCleared`.** The `about_to_wait()`
   callback is called when the event loop is about to block and wait for new
   events. This is the correct place to check timeouts and request redraws.

### WindowedStaticApp Implementation

```rust
struct WindowedStaticApp {
    renderer_id: RendererId,
    viewport: RendererViewport,
    loaded_state: Option<LoadedStaticImageState>,
    timeout: Option<Duration>,
    start: Instant,
    window: Option<winit::window::Window>,
}

impl ApplicationHandler for WindowedStaticApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) { ... }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) { ... }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) { ... }
}
```

The `window` field is `Option<Window>` because the window does not exist at
construction time — it is created in `resumed()`. If window creation fails
(e.g., no display server), the handler logs the error and calls
`event_loop.exit()` to shut down gracefully without panicking.

The event loop is started with `event_loop.run_app(&mut app)`, which blocks
until the event loop exits.

## Viewport Resize Handling and Layout Recalculation

One of the critical behaviors validated by the windowed static renderer is
viewport resize handling with automatic layout recalculation. When the user
resizes the window (or when a monitor resolution change occurs), the renderer
must:

1. **Detect the resize.** The `WindowEvent::Resized(physical_size)` event
   provides the new dimensions in physical pixels. The handler checks that both
   `width` and `height` are non-zero before proceeding (zero-dimension events
   can occur during minimization on some platforms).

2. **Update the viewport.** The `RendererViewport` is updated with the new
   dimensions. This is the renderer's source of truth for layout calculation.

3. **Recalculate layout.** If a wallpaper is currently applied
   (`loaded_state` is `Some`), the layout is recalculated using
   `calculate_static_image_layout()` with the new viewport dimensions. The
   fit mode and background color are preserved from the original application.
   The `loaded_state` is then replaced with the new layout.

4. **Log the recalculation.** The new destination rectangle dimensions and
   viewport are logged for diagnostics. This is essential for debugging
   layout issues on real hardware.

5. **Handle failure gracefully.** If the layout recalculation fails (e.g.,
   due to a zero-dimension viewport that somehow passed the guard check), the
   handler logs a warning but does not crash or exit. The old layout remains
   in effect.

In the current implementation, the recalculated layout is not yet rendered to
the window surface (pending wgpu integration). However, the full data flow —
resize → viewport update → layout recalculation → state update — is exercised
and validated. When wgpu is added, the `about_to_wait()` callback will trigger
a redraw that uses the updated layout.

## RenderSimReport Structure

The `RenderSimReport` is the primary output of the `--headless-render-sim`
mode. It is a self-contained JSON structure that captures everything that
happened during a render simulation run, enabling automated validation in CI.

```rust
pub struct RenderSimReport {
    pub mode: RendererRuntimeMode,
    pub viewport: RendererViewport,
    pub state_transitions: Vec<RendererRuntimeState>,
    pub wallpaper_applied: bool,
    pub layout_report: Option<RenderSimLayoutReport>,
    pub total_sim_time_ms: u64,
    pub exit_code: i32,
}
```

### Fields

- **`mode`**: Always `HeadlessRenderSim`. Included for identification when the
  report is consumed by automated tooling.

- **`viewport`**: The viewport dimensions used for the simulation. If the
  viewport is invalid (zero dimensions), the report is still produced but with
  `exit_code: 1` and `state_transitions: [Starting, Failed]`.

- **`state_transitions`**: An ordered list of every `RendererRuntimeState`
  transition that occurred during the simulation. A healthy run produces:
  `[Starting, Ready, Running, ShuttingDown, Exited]`. A failed run may
  produce `[Starting, Failed]` or `[Starting, Ready, Running, Failed]`.

- **`wallpaper_applied`**: `true` if a wallpaper source image was provided
  (via `--source`) and was successfully decoded and laid out. `false` if no
  source was provided or if the apply failed.

- **`layout_report`**: If a wallpaper was applied, this contains the full
  layout calculation result including image dimensions, viewport dimensions,
  and the destination rectangle (x, y, width, height). This enables
  automated verification of layout correctness without visual inspection.

- **`total_sim_time_ms`**: Total wall-clock time of the simulation in
  milliseconds. Useful for performance regression detection.

- **`exit_code`**: `0` for success, `1` for failure. A failure occurs when
  the viewport is invalid or when the simulation encounters an unrecoverable
  error.

### RenderSimLayoutReport

```rust
pub struct RenderSimLayoutReport {
    pub image_width: u32,
    pub image_height: u32,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub destination_x: f64,
    pub destination_y: f64,
    pub destination_width: f64,
    pub destination_height: f64,
}
```

This sub-report captures the layout calculation result. The `destination_*`
fields represent the rectangle where the image should be drawn within the
viewport, computed by `calculate_static_image_layout()`. The `render-sim-smoke`
test verifies that these values are non-zero and consistent with the expected
fit mode behavior (e.g., cover mode fills the viewport with possible overflow).

### Example Report

```json
{
  "mode": "HeadlessRenderSim",
  "viewport": { "width": 800, "height": 450, "scale_factor": null },
  "state_transitions": ["Starting", "Ready", "Running", "ShuttingDown", "Exited"],
  "wallpaper_applied": true,
  "layout_report": {
    "image_width": 2,
    "image_height": 2,
    "viewport_width": 800,
    "viewport_height": 450,
    "destination_x": 0.0,
    "destination_y": -337.5,
    "destination_width": 800.0,
    "destination_height": 1125.0
  },
  "total_sim_time_ms": 2005,
  "exit_code": 0
}
```

This report shows a 2×2 image laid out in cover mode on an 800×450 viewport.
The destination rectangle (0, −337.5, 800, 1125) extends beyond the viewport
boundaries vertically, which is correct for cover mode — the image is scaled
to fill the viewport and the overflow is clipped.
