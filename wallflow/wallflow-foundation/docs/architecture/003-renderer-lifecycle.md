# 003 – Renderer Lifecycle & Supervisor

> Stage: `003-cloud-safe-core-renderer-integration`
> Status: Implemented
> Date: 2026-05-30

## Overview

This document describes the renderer lifecycle model, the `RendererSupervisor`
that manages renderer processes, and the heartbeat-based health monitoring
system. All components are **cloud-testable**: they do not depend on a real
Windows desktop or GUI.

## Architecture

### Process Model

```
┌──────────────────────────────────────────┐
│                  CoreApp                  │
│                                          │
│  ┌─────────────┐   ┌──────────────────┐  │
│  │  Renderer    │   │  MonitorProvider │  │
│  │  Supervisor  │   │  (platform)      │  │
│  │              │   └──────────────────┘  │
│  │  ┌────────┐  │                        │
│  │  │Entry 1 │  │   Each entry:          │
│  │  │Entry 2 │  │   - RendererAssignment │
│  │  │  ...   │  │   - RendererStatus     │
│  │  └────────┘  │   - ProcessManager     │
│  └─────────────┘   - Heartbeat tracking  │
│                     - Restart counter     │
└──────────────────────────────────────────┘
          │                     │
          ▼                     ▼
   ┌──────────────┐    ┌──────────────┐
   │ Renderer #1  │    │ Renderer #2  │
   │ (headless /  │    │ (headless /  │
   │  desktop)    │    │  desktop)    │
   └──────────────┘    └──────────────┘
```

### Renderer State Machine

```
         ┌──────────┐
         │ Starting │◄──────── recover()
         └────┬─────┘
              │ mark_running() / mark_heartbeat()
              ▼
         ┌──────────┐
    ┌───►│ Running  │◄─── mark_resumed()
    │    └──┬───┬───┘
    │       │   │ mark_paused()
    │       │   ▼
    │       │ ┌──────────┐
    │       │ │  Paused  │
    │       │ └──────────┘
    │       │
    │  detect_stale()
    │       │
    │       ▼
    │    ┌──────────┐
    │    │  Stale   │
    │    └──────────┘
    │
    │  mark_stopping()
    │       │
    │       ▼
    │    ┌──────────┐       mark_crashed()
    │    │ Stopping │──────────────┐
    │    └────┬─────┘              │
    │         │                    ▼
    │         ▼              ┌──────────┐
    │    ┌──────────┐        │ Crashed  │──► should_restart()? ──► recover()
    │    │ Stopped  │        └────┬─────┘
    │    └──────────┘             │
    │                        exceeds limit
    │                             │
    │                             ▼
    │                        ┌──────────┐
    │                        │ SafeMode │
    │                        └──────────┘
    │
    └── mark_heartbeat() from Stale ──► Running
```

## Key Types

### RendererStatus (wallflow-core)

| Status | Meaning |
|--------|---------|
| Starting | Process spawned, waiting for first heartbeat |
| Running | Active and sending heartbeats |
| Stale | Heartbeat timeout exceeded |
| Paused | Paused by user request |
| Stopping | Graceful shutdown in progress |
| Stopped | Clean shutdown complete |
| Crashed | Process exited unexpectedly; eligible for restart |
| SafeMode | Exceeded max restarts; core must enter safe mode |

### RendererHealth (wallflow-common)

| Health | Meaning |
|--------|---------|
| Healthy | Heartbeat received within timeout |
| Stale | No recent heartbeat, but within restart window |
| Unhealthy | Exceeded max restarts or in safe mode |

### RendererRestartPolicy (wallflow-common)

| Policy | Behavior |
|--------|----------|
| Never | Never restart after a crash |
| Limited { max_attempts } | Restart up to N times within the window |
| Always | Always restart regardless of crash count |

### WatchdogPolicy (wallflow-core)

| Field | Default | Description |
|-------|---------|-------------|
| heartbeat_timeout_secs | 5 | Seconds without heartbeat before marking stale |
| max_restarts_per_window | 3 | Max restarts before safe mode |
| restart_window_secs | 60 | Rolling window for restart counting |

## IPC Protocol

### RendererCommand (Core → Renderer)

| Command | Purpose |
|---------|---------|
| Start | Begin rendering |
| Pause | Pause (keep resources) |
| Resume | Resume from pause |
| Stop | Stop and release resources |
| ApplyWallpaper | Change wallpaper assignment |
| SetMonitor | Reassign to different monitor |
| Shutdown | Graceful shutdown |

### RendererEvent (Renderer → Core)

| Event | Purpose |
|-------|---------|
| Started | Process started |
| Ready | Initialization complete |
| Heartbeat | Periodic liveness signal |
| Paused | Confirmed pause |
| Resumed | Confirmed resume |
| WallpaperApplied | Wallpaper is rendering |
| Error | Error occurred |
| Exited | Process exiting |

### CoreCommand (External → Core)

| Command | Purpose |
|---------|---------|
| ApplyWallpaperToMonitor | Assign wallpaper to monitor |
| StopWallpaper | Stop wallpaper on monitor |
| PauseAll | Pause all renderers |
| ResumeAll | Resume all renderers |
| QueryState | Query all renderer states |
| GetMonitors | Get monitor snapshot |
| EnterSafeMode | Force safe mode |
| ExitSafeMode | Leave safe mode |
| Shutdown | Shut down core |

### CoreEvent (Core → Listeners)

| Event | Purpose |
|-------|---------|
| StateChanged | Core state changed |
| RendererStarted | New renderer started |
| RendererStopped | Renderer stopped normally |
| RendererCrashed | Renderer crashed |
| RendererRecovered | Renderer recovered after crash |
| MonitorsSnapshot | Monitor information |
| Ready | Core is ready |
| Error | Core error |
| SafeModeChanged | Safe mode toggled |

Protocol version is now **2** (bumped from 1 due to new command/event types).

## Headless Heartbeat Mode

The renderer can run without any GUI or Win32 dependencies:

```bash
cargo run -p wallflow-renderer -- --headless-heartbeat --heartbeat-interval-ms 500 --timeout-secs 5
```

This mode:
- Starts without creating any windows
- Periodically emits JSON heartbeat events on stdout
- Exits cleanly after the specified timeout
- Returns exit code 0 on success
- Is fully testable on Linux/CI

## Supervisor Smoke Test

```bash
cargo run -p wallflow-cli -- supervisor-smoke --timeout-secs 5 --heartbeat-interval-ms 500
```

This command:
1. Finds the renderer executable in the build output
2. Spawns it in headless heartbeat mode
3. Reads stdout lines as heartbeat events
4. Validates that Started, Ready, Heartbeat, and Exited events are received
5. Prints a structured JSON report
6. Returns exit code 0 on success

## Cloud-Testable vs. Windows-Only

| Component | Cloud-Testable | Windows-Only | REQUIRES_REAL_WINDOWS_VALIDATION |
|-----------|:-:|:-:|:-:|
| RendererSupervisor | ✅ | | |
| RendererProcessManager | ✅ | | |
| WatchdogPolicy / decisions | ✅ | | |
| IPC protocol types | ✅ | | |
| Headless heartbeat mode | ✅ | | |
| supervisor-smoke command | ✅ | | |
| All unit tests | ✅ | | |
| desktop-probe | | ✅ | ✅ |
| desktop-attach-smoke | | ✅ | ✅ |
| Desktop attach renderer | | ✅ | ✅ |
| Explorer restart tolerance | | | ✅ |
| Multi-monitor attach | | | ✅ |

## Test Coverage

| Area | Tests | Count |
|------|-------|-------|
| Watchdog decisions | wallflow-core/watchdog | 10 |
| Supervisor lifecycle | wallflow-core/supervisor | 15 |
| IPC protocol roundtrips | wallflow-ipc/protocol | 6 |
| IPC frame roundtrips | wallflow-ipc/frame | 1 |
| Desktop attach (stubs) | wallflow-desktop | 11 |
| Monitor diff | wallflow-monitor/diff | 3 |
| Config | wallflow-config | 2 |
| Media backend | wallflow-media | 2 |
| **Total** | | **50** |
