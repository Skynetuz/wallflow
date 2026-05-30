# Windows Desktop Attach

## Overview

WallFlow places live wallpaper windows behind the desktop icons by embedding a renderer window into the Windows desktop window hierarchy. This is the same technique used by Wallpaper Engine and other live wallpaper applications.

## Window Hierarchy

```text
Progman (Program Manager)
├── WorkerW (spawned via message 0x052C)
│   └── <WallFlow renderer window>  ← attached here
├── SHELLDLL_DefView
│   └── SysListView32 (desktop icons)
└── WorkerW (default, contains icon layer)
```

The key insight is that Windows Explorer creates a specific `WorkerW` window that sits behind the `SHELLDLL_DefView` (which hosts the desktop icons). By attaching a renderer window as a child of this `WorkerW`, the wallpaper appears behind the icons but above the default desktop color.

## Discovery Algorithm

The `wallflow-desktop` crate implements the following discovery algorithm:

1. **Find Progman**: Call `FindWindowW("Progman", null)` to locate the Program Manager window.
2. **Spawn WorkerW**: Send message `0x052C` to Progman via `SendMessageTimeoutW`. This causes Windows to create a new `WorkerW` child window that will host our renderer.
3. **Enumerate windows**: Use `EnumWindows` to iterate top-level windows:
   - For each window, check if it has a child named `SHELLDLL_DefView`.
   - If found, look for a sibling `WorkerW` window that appears after the current window in Z-order.
   - The `WorkerW` sibling of `SHELLDLL_DefView` is our target.
4. **Attach**: Use `SetParent(renderer_hwnd, workerw_hwnd)` to embed the renderer.

## API

### `probe_desktop() -> DesktopProbeReport`

Non-mutating diagnostic function that reports the current state of the desktop window hierarchy. Returns:
- Whether the platform is supported
- HWNDs of Progman, SHELLDLL_DefView, and WorkerW
- Whether attach is feasible
- Error details if something failed

### `find_desktop_worker() -> Result<DesktopWorkerHandle, DesktopError>`

Finds the target WorkerW window for wallpaper rendering. This calls `probe_desktop()` internally and returns just the handle.

### `attach_window_to_desktop(window) -> Result<DesktopAttachReport, DesktopError>`

Parents a renderer window into the WorkerW. Returns diagnostic information including the previous parent HWND.

### `detach_window_from_desktop(window) -> Result<DesktopDetachReport, DesktopError>`

Removes a renderer window from the desktop hierarchy by reparenting it to the desktop (null parent). Must be called before destroying the renderer window.

## Error Handling

All functions return `Result<_, DesktopError>` with typed errors:
- `ProgmanNotFound`: Progman window could not be found
- `WorkerWSpawnFailed`: The 0x052C message did not produce a WorkerW
- `WorkerWindowNotFound`: No suitable WorkerW found during enumeration
- `ShellDefViewNotFound`: SHELLDLL_DefView not found (unusual)
- `AttachFailed(String)`: SetParent failed with details
- `DetachFailed(String)`: Detach SetParent failed with details
- `InvalidHandle`: Null window handle passed
- `UnsupportedPlatform(String)`: Running on non-Windows OS

## CLI Commands

### `desktop-probe`

```powershell
cargo run -p wallflow-cli -- desktop-probe
```

Outputs a JSON diagnostic report and a human-readable summary. This is the first command to run when debugging desktop attach issues.

### `desktop-attach-smoke`

```powershell
cargo run -p wallflow-cli -- desktop-attach-smoke
```

End-to-end test that:
1. Probes the desktop hierarchy
2. Creates a dummy Win32 window
3. Attaches it behind desktop icons
4. Waits 5 seconds for visual verification
5. Detaches and destroys the window

### Renderer `--desktop-attach` mode

```powershell
cargo run -p wallflow-renderer -- --desktop-attach --timeout-secs 30
```

Starts a standalone renderer process with a dummy window that attaches to the desktop. Use `--timeout-secs 0` to run until Ctrl+C.

## Explorer Restart Tolerance

If Explorer restarts, the WorkerW handle becomes invalid. The current implementation does not automatically recover from this scenario. The recommended approach for the next stage:

1. Periodically validate the WorkerW handle with `IsWindow()`.
2. If the handle becomes invalid, re-probe the desktop hierarchy.
3. Re-attach the renderer window to the new WorkerW.
4. If re-attachment fails after N attempts, enter safe mode.

## Structured Logging

All desktop operations emit structured logs via `tracing`:
- `info!` for successful discoveries and attachments
- `debug!` for enumeration details
- `warn!` for failures and missing windows

Enable verbose logging with:
```powershell
$env:RUST_LOG = "wallflow_desktop=debug"
```

## Manual Test Procedure

### Windows 10 22H2 — Single Monitor

1. Run `cargo run -p wallflow-cli -- desktop-probe`
   - Expected: `progman_hwnd` is non-zero, `workerw_hwnd` is non-zero, `attach_feasible` is true
2. Run `cargo run -p wallflow-cli -- desktop-attach-smoke`
   - Expected: A window appears behind desktop icons for 5 seconds, then disappears cleanly
3. Check that desktop icons are still clickable after the smoke test

### Windows 11 22H2+ — Single Monitor

1. Same procedure as Windows 10
2. Note: Windows 11 may have different WorkerW behavior with the new taskbar

### Multi-Monitor

1. Connect two monitors
2. Run `desktop-probe` — should still find a valid WorkerW
3. Run `desktop-attach-smoke` — window should appear on the primary monitor
4. Check that both monitors' icons remain functional

### Explorer Restart

1. Run `cargo run -p wallflow-renderer -- --desktop-attach --timeout-secs 60`
2. While running, kill Explorer via Task Manager
3. Restart Explorer (File → Run new task → `explorer.exe`)
4. Observe: the renderer window will likely become orphaned or invisible
5. This is a known limitation — automatic recovery is planned for stage 003

### Application Exit

1. Run `desktop-attach-smoke`
2. Verify the window disappears cleanly after the test
3. No residual window should remain in the desktop hierarchy
4. Desktop icons should be fully functional

## Known Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| 0x052C message behavior may change in future Windows builds | High | The technique has been stable since Windows 7; monitor Windows Insider builds |
| Multiple wallpaper engines running simultaneously | Medium | WorkerW discovery may find the wrong window; consider checking parent chains |
| Explorer restart invalidates WorkerW handle | Medium | Implement periodic handle validation and re-attachment (stage 003) |
| Windows N editions without Media Feature Pack | Low | Does not affect desktop attach; only media playback |
| DPI/per-monitor DPI changes | Low | Renderer window should handle `WM_DPICHANGED`; not yet implemented |

## Next Steps (Stage 003)

1. Implement periodic WorkerW handle validation
2. Add Explorer restart detection and automatic re-attachment
3. Implement proper renderer window sizing to match monitor work area
4. Add `WM_DPICHANGED` handling for DPI awareness
5. Handle multi-monitor: create one renderer per monitor
6. Integrate with `wallflow-core` orchestration lifecycle
7. Add winit/wgpu rendering pipeline for actual content
