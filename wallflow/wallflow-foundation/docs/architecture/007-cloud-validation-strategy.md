# 007 – Cloud Validation Strategy

> Stage: `003-cloud-safe-core-renderer-integration`
> Date: 2026-05-30

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
- RendererAssignment and monitor mapping
- Monitor diff detection
- Configuration loading/saving
- Headless renderer heartbeat mode
- Supervisor smoke test (process-based)

### Tier 2: Windows Compile-Check (CI)

Code that compiles on Windows but does not require an interactive desktop.
Validated via GitHub Actions on `windows-latest`.

- Full workspace `cargo check --workspace` on Windows
- Full workspace `cargo test --workspace` on Windows
- Unit tests that use `cfg(windows)` stubs

### Tier 3: REQUIRES_REAL_WINDOWS_VALIDATION

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
┌─────────────────────────────────┐
│        Ubuntu Runner            │
│                                 │
│  1. cargo fmt --all --check     │
│  2. cargo check --workspace     │
│  3. cargo clippy (strict)       │
│  4. cargo test --workspace      │
│  5. headless heartbeat smoke    │
│  6. supervisor smoke            │
└─────────────────────────────────┘

┌─────────────────────────────────┐
│        Windows Runner           │
│                                 │
│  1. cargo check --workspace     │
│  2. cargo test --workspace      │
│                                 │
│  (No desktop-attach smoke:      │
│   runner lacks interactive      │
│   Explorer desktop session)     │
└─────────────────────────────────┘
```

## Headless Testing Pattern

The headless heartbeat mode enables a powerful testing pattern:

1. **Renderer**: Run with `--headless-heartbeat --timeout-secs N` — emits
   JSON events on stdout, no GUI needed.
2. **Supervisor/CLI**: Spawn the renderer as a subprocess, read stdout,
   parse events, validate lifecycle.
3. **Integration**: The supervisor-smoke command wraps this pattern and
   produces a structured JSON report.

This pattern can be extended:
- Replace stdout-based events with IPC pipe events once the IPC layer is
  connected to the renderer process.
- Add latency measurements, error injection, and crash recovery scenarios.

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

- **Stage 004**: Connect IPC layer to renderer process (replace stdout
  heartbeat with typed IPC frames)
- **Stage 005**: Implement winit/wgpu-based rendering loop
- **Stage 006**: Real Windows validation with manual testing on Win10/11
