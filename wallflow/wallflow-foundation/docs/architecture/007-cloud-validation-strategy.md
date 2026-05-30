# 007 – Cloud Validation Strategy

> Stage: `004-cloud-safe-typed-ipc-renderer-control`
> Date: 2026-05-30 (updated)

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
full IPC command/event lifecycle via piped stdio.

- `--ipc-stdio` renderer mode (typed IPC frames over stdin/stdout)
- `ipc-supervisor-smoke` CLI command (Start → Ready → Heartbeat → Pause →
  Paused → Resume → Resumed → Shutdown → Exited)
- `--headless-heartbeat` renderer mode (legacy stdout text mode)

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

- **Stage 005**: Implement winit/wgpu-based rendering loop
- **Stage 006**: Real Windows validation with manual testing on Win10/11
