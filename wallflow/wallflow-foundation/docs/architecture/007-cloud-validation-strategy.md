# 007 – Cloud Validation Strategy

> Stage: `004-cloud-safe-typed-ipc-renderer-control`
> Date: 2026-05-30 (updated 2026-06-01)

## Problem

WallFlow is a Windows-first live wallpaper engine, but development and CI
primarily run on Linux. This document establishes a strategy for validating as
much as possible in the cloud while clearly marking what requires a real
Windows machine.

## Validation Tiers

### Tier 1: Cloud-Testable (Linux/CI)

All pure logic, data structures, state machines, and protocol types. These
are tested with standard `cargo test` on any platform.

- RendererSupervisor lifecycle methods
- WatchdogPolicy decisions (fresh/stale/safe mode)
- RendererRestartPolicy evaluation
- IPC serialization/deserialization roundtrips
- IPC frame encoding/decoding (sync and async)
- Protocol version validation
- CommandEnvelope / EventEnvelope construction
- IpcMessage tagged union serialization
- RendererAssignment and monitor mapping
- Monitor diff detection
- Configuration loading/saving

### Tier 2: Cloud-Testable Integration (Linux/CI process smoke)

Integration tests that spawn actual renderer processes and validate the
full IPC command/event lifecycle or the full renderer lifecycle via piped stdio.

- `--ipc-stdio` renderer mode (typed IPC frames over stdin/stdout)
- `ipc-supervisor-smoke` CLI command (Start → Ready → Heartbeat → Pause →
  Paused → Resume → Resumed → Shutdown → Exited)
- `--headless-heartbeat` renderer mode (legacy stdout text mode)
- `--headless-render-sim` renderer mode (full lifecycle + layout, no display server)
- `render-sim-smoke` CLI command (renderer lifecycle + wallpaper apply + layout verification)

### Tier 3: Windows Compile-Check (CI)

Code that compiles on Windows but does not require an interactive desktop.
Validated via GitHub Actions on `windows-latest`.

- Full workspace `cargo check --workspace` on Windows
- Full workspace `cargo test --workspace` on Windows
- Unit tests that use `cfg(windows)` stubs

### Tier 4: REQUIRES_REAL_WINDOWS_VALIDATION

Code that depends on the actual Windows desktop environment: Explorer,
Progman, WorkerW, and the full Win32 window hierarchy.

- `desktop-probe` (finding Progman/WorkerW/SHELLDLL_DefView)
- `desktop-attach-smoke` (creating a window, attaching behind icons)
- `--desktop-attach` renderer mode (long-running embedded window)
- Explorer restart tolerance
- Multi-monitor desktop attach
- DPI change handling (WM_DPICHANGED)
- Full-screen application detection

These must be tested on a physical or VM Windows 10 22H2+ / Windows 11
machine with an interactive desktop session.

## Cross-Compilation Results

| Target | Result | Reason |
|--------|--------|--------|
| `x86_64-pc-windows-gnu` | Partial | Pure-logic crates compile; Win32 crates fail because GNU linker lacks Windows import libraries |
| `x86_64-pc-windows-msvc` | Partial | Same issue: MSVC linker and Windows SDK libs are not available in the Linux environment |
| `x86_64-unknown-linux-gnu` | Full | All workspace crates compile and test cleanly |

**Conclusion**: Cross-compiling from Linux to Windows is not feasible without
a proper Windows SDK cross-compilation setup (e.g., `xwin` or `cargo-xwin`).
The GitHub Actions Windows runner provides a much simpler solution.

## CI Strategy

```
┌──────────────────────────────────────────┐
│        Ubuntu Runner (lint + test)        │
│                                           │
│  1. cargo fmt --all -- --check            │
│  2. cargo check --workspace               │
│  3. cargo clippy (strict)                 │
│  4. cargo test --workspace (67 tests)      │
└──────────────────────────────────────────┘

┌──────────────────────────────────────────┐
│        Ubuntu Runner (IPC smoke)          │
│                                           │
│  1. cargo build -p wallflow-renderer      │
│  2. cargo build -p wallflow-cli           │
│  3. ipc-supervisor-smoke (typed IPC)      │
│  4. legacy headless heartbeat smoke       │
└──────────────────────────────────────────┘

┌──────────────────────────────────────────┐
│        Windows Runner                     │
│                                           │
│  1. cargo check --workspace               │
│  2. cargo test --workspace                │
│                                           │
│  (No desktop-attach smoke:                │
│   runner lacks interactive                │
│   Explorer desktop session)               │
└──────────────────────────────────────────┘
```

## IPC Testing Pattern

The `--ipc-stdio` mode enables a powerful testing pattern:

1. **Renderer**: Run with `--ipc-stdio --heartbeat-interval-ms N --timeout-secs T`.
   Reads typed `RendererCommand` frames from stdin, writes typed
   `RendererEvent` frames to stdout. All logs go to stderr.
2. **Supervisor/CLI**: Spawn the renderer as a subprocess, pipe stdin/stdout,
   exchange typed IPC frames, validate the full lifecycle.
3. **Integration**: The `ipc-supervisor-smoke` command wraps this pattern and
   produces a structured JSON report.

This pattern can be extended:
- Add latency measurements, error injection, and crash recovery scenarios.
- Replace stdio pipes with Windows named pipes or Unix domain sockets.
- Add protocol fuzzing and malformed frame handling tests.

## Render Simulation Smoke Test (render-sim-smoke)

The `render-sim-smoke` CLI command is a new cloud-testable integration test
that exercises the full renderer lifecycle without requiring a display server.
It goes beyond the `ipc-supervisor-smoke` test by validating not just IPC
communication, but also the internal renderer runtime — image decode, layout
calculation, state machine transitions, and structured report generation.

### How It Works

1. **Create a test wallpaper package.** The CLI creates a temporary directory
   with a `manifest.json` and a real 2×2 PNG image (generated using the `image`
   crate, not a dummy file). This ensures the full image decode path is
   exercised.

2. **Spawn the renderer in `--headless-render-sim` mode.** The renderer is
   launched with `--headless-render-sim --width W --height H --source PATH
   --timeout-secs T`. The renderer:
   - Validates the viewport dimensions.
   - Transitions through Starting → Ready → Running.
   - Decodes the image metadata using `wallflow_package::load_image_metadata()`.
   - Calculates the static image layout for the given viewport and fit mode.
   - Simulates the running period for the specified timeout.
   - Transitions through ShuttingDown → Exited.
   - Outputs a `RenderSimReport` as JSON on stdout.

3. **Capture and parse the report.** The CLI reads the JSON report from the
   renderer's stdout. Since `--headless-render-sim` directs all logs to stderr,
   stdout contains only the report JSON — no log noise.

4. **Verify the report.** The CLI checks:
   - `exit_code` is `0`.
   - `state_transitions` includes the expected sequence:
     `[Starting, Ready, Running, ShuttingDown, Exited]`.
   - `wallpaper_applied` is `true`.
   - `layout_report` is present and contains valid dimensions.
   - The destination rectangle has non-zero width and height.
   - The image dimensions match the expected 2×2 test image.
   - The viewport dimensions match the `--width` and `--height` arguments.

5. **Print a structured summary.** The CLI outputs a human-readable summary
   and a JSON report of the test results, consistent with the other smoke
   test commands.

### Why This Is a Smoke Test, Not a Unit Test

The `render-sim-smoke` test spawns an actual renderer process, which means it
exercises:

- Process spawning and argument parsing.
- The full renderer `main()` function dispatch to `run_headless_render_sim()`.
- Image file I/O (reading a real PNG from disk).
- Layout calculation with the full `wallflow_package` pipeline.
- JSON serialization of the `RenderSimReport`.
- Clean process exit with code 0.

These are integration concerns that cannot be tested with `cargo test` unit
tests alone. The renderer process is a black box — the smoke test validates
its observable behavior (stdout output, exit code) without importing any
internal modules.

### Running the Test

```bash
# Build the renderer and CLI first
cargo build -p wallflow-renderer -p wallflow-cli

# Run with default settings (5s timeout, 800×450 viewport)
cargo run -p wallflow-cli -- render-sim-smoke

# Run with custom viewport and timeout
cargo run -p wallflow-cli -- render-sim-smoke --timeout-secs 3 --width 1920 --height 1080
```

### What It Validates vs. What It Does Not

| Validated | Not validated |
|-----------|---------------|
| Renderer process spawns and exits cleanly | Actual pixel rendering |
| State machine transitions occur in correct order | Window creation or display server interaction |
| Image metadata decode succeeds | GPU surface creation |
| Layout calculation produces valid rectangles | Visual correctness of fit modes |
| RenderSimReport serialization roundtrips correctly | DPI scale factor handling |
| Wallpaper apply path works without IPC | IPC command handling (use `ipc-supervisor-smoke` for that) |
| Renderer handles invalid viewports gracefully | Resize handling (use `--windowed-static` for that) |

### CI Integration

The `render-sim-smoke` command should be added to the Ubuntu CI runner after
the existing IPC smoke tests:

```
┌──────────────────────────────────────────┐
│        Ubuntu Runner (IPC + render sim)   │
│                                           │
│  1. cargo build -p wallflow-renderer      │
│  2. cargo build -p wallflow-cli           │
│  3. ipc-supervisor-smoke (typed IPC)      │
│  4. legacy headless heartbeat smoke       │
│  5. render-sim-smoke (full lifecycle)     │
└──────────────────────────────────────────┘
```

This test runs in seconds and requires no special hardware, display server,
or GPU — making it ideal for every pull request and merge to main.

## Marking Convention

All code, tests, and documentation that cannot be validated without a real
Windows desktop must be marked with:

```
REQUIRES_REAL_WINDOWS_VALIDATION
```

This marker appears in:
- Source code comments above `#[cfg(target_os = "windows")]` blocks that
  interact with the desktop window hierarchy
- Documentation sections about desktop attach
- Test descriptions for desktop-probe and desktop-attach-smoke
- CI workflow comments explaining why certain tests are skipped

## Next Steps

- **Stage 008**: Add wgpu rendering pipeline to the windowed static renderer
- **Stage 009**: Connect desktop attach to winit window on Windows
- **Stage 010**: Real Windows validation with manual testing on Win10/11
