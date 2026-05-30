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
5. **Size**: After `SetParent`, call `GetClientRect` on the WorkerW and `MoveWindow` on the renderer to fill the entire desktop area.

## Window Style

The renderer window must be created with `WS_POPUP | WS_VISIBLE` style (no title bar or borders). This is critical because:

- `WS_OVERLAPPEDWINDOW` (title bar + borders) causes visual artifacts when embedded in WorkerW.
- `WS_POPUP` creates a borderless window that can fill the entire desktop.
- After `SetParent`, the window is repositioned with `MoveWindow` to fill the WorkerW client area.

## Background Painting

The renderer window handles `WM_ERASEBKGND` by painting a solid dark blue background (BGR `0x804040`). This makes the window clearly visible behind desktop icons during testing. In production, the winit/wgpu pipeline will replace this with actual wallpaper content.

## Message Loop

Both the CLI smoke test and the renderer process use `PeekMessageW` with `PM_REMOVE` in a loop with `Sleep(50ms)` fallback, rather than blocking `GetMessageW`. This is necessary because:

- `GetMessageW` blocks indefinitely when no messages arrive, preventing timeout checks.
- `PeekMessageW` returns immediately, allowing the loop to check elapsed time.
- The 50ms sleep between empty polls avoids busy-waiting.

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

Parents a renderer window into the WorkerW. Returns diagnostic information including the previous parent HWND. Validates the child window with `IsWindow()` before calling `SetParent`.

### `detach_window_from_desktop(window) -> Result<DesktopDetachReport, DesktopError>`

Removes a renderer window from the desktop hierarchy by reparenting it to the desktop (null parent). Must be called before destroying the renderer window. Validates the child window with `IsWindow()` before calling `SetParent`.

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
2. Creates a dummy Win32 popup window (full screen size)
3. Attaches it behind desktop icons via `SetParent`
4. Sizes it to fill the WorkerW client area via `MoveWindow`
5. Runs a 5-second message loop with `PeekMessageW`
6. Detaches via `SetParent(null)`
7. Destroys the window

### Renderer `--desktop-attach` mode

```powershell
cargo run -p wallflow-renderer -- --desktop-attach --timeout-secs 30
```

Starts a standalone renderer process with a dummy window that attaches to the desktop. Uses `PeekMessageW` loop with timeout support. Use `--timeout-secs 0` to run until Ctrl+C.

## Explorer Restart Tolerance

If Explorer restarts, the WorkerW handle becomes invalid. The current implementation does not automatically recover from this scenario. The recommended approach for the next stage:

1. Periodically validate the WorkerW handle with `IsWindow()`.
2. If the handle becomes invalid, re-probe the desktop hierarchy.
3. Re-attach the renderer window to the new WorkerW.
4. If re-attachment fails after N attempts, enter safe mode.

**Observed behavior** (requires real Windows validation):
- When Explorer is killed, the WorkerW window is destroyed.
- The attached renderer window becomes orphaned (its parent no longer exists).
- When Explorer restarts, a new Progman/WorkerW hierarchy is created.
- The old renderer window may still be visible but unresponsive.
- `IsWindow(workerw_hwnd)` will return FALSE after Explorer restart.

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
   - Expected: A dark blue rectangle appears behind desktop icons for 5 seconds, then disappears cleanly
3. Check that desktop icons are still clickable after the smoke test

### Windows 11 22H2+ — Single Monitor

1. Same procedure as Windows 10
2. Note: Windows 11 may have different WorkerW behavior with the new taskbar

### Multi-Monitor

1. Connect two monitors
2. Run `desktop-probe` — should still find a valid WorkerW
3. Run `desktop-attach-smoke` — window should appear on the primary monitor
4. Check that both monitors' icons remain functional
5. Note: The current implementation sizes the window to `GetSystemMetrics(SM_CXSCREEN)` which only covers the primary monitor. Multi-monitor support requires per-monitor sizing in stage 003.

### Explorer Restart

1. Run `cargo run -p wallflow-renderer -- --desktop-attach --timeout-secs 60`
2. While running, kill Explorer: `taskkill /f /im explorer.exe`
3. Restart Explorer: `start explorer.exe`
4. Observe: the renderer window will likely become orphaned or invisible
5. Check if the renderer process is still running
6. This is a known limitation — automatic recovery is planned for stage 003

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
| Window sizing only covers primary monitor | Medium | Need per-monitor sizing with `EnumDisplayMonitors` (stage 003) |
| Windows N editions without Media Feature Pack | Low | Does not affect desktop attach; only media playback |
| DPI/per-monitor DPI changes | Low | Renderer window should handle `WM_DPICHANGED`; not yet implemented |
| PeekMessageW loop uses 50ms polling interval | Low | Acceptable for MVP; stage 003 may use MsgWaitForMultipleObjectsEx |

## Next Steps (Stage 003)

1. Implement periodic WorkerW handle validation
2. Add Explorer restart detection and automatic re-attachment
3. Implement proper renderer window sizing to match monitor work area
4. Add `WM_DPICHANGED` handling for DPI awareness
5. Handle multi-monitor: create one renderer per monitor
6. Integrate with `wallflow-core` orchestration lifecycle
7. Add winit/wgpu rendering pipeline for actual content
