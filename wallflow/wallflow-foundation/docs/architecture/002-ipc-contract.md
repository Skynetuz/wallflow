# 002 – IPC Contract

> Stage: `004-cloud-safe-typed-ipc-renderer-control`
> Date: 2026-05-30

## Overview

This document specifies the IPC (Inter-Process Communication) contract between
the Core process and Renderer processes in WallFlow. The contract defines the
wire format, message types, versioning strategy, and transport mechanism.

## Transport: Stdio Pipes

For the cloud-testable MVP, IPC uses **piped stdio**:

- **Core → Renderer**: Core writes IPC frames to the renderer's stdin pipe.
- **Renderer → Core**: Renderer writes IPC frames to its stdout pipe.
- **Logs**: Renderer writes diagnostic logs to stderr only (never to stdout).

This design is chosen because:

1. **Cloud-testable**: Works on Linux CI without any platform-specific setup.
2. **Simple**: No socket files, named pipes, or port management needed.
3. **Process-model friendly**: Stdin/stdout pipes are the natural communication
   channel for child processes spawned by a supervisor.
4. **Swappable**: The transport layer can be replaced with Windows named pipes
   or Unix domain sockets later without changing the protocol types or framing
   logic. Only the read/write endpoints change.

### Why logs go to stderr

In `--ipc-stdio` mode, the renderer's stdout is reserved exclusively for IPC
frames. Any diagnostic output (tracing, warnings, errors) must go to stderr.
This prevents log lines from corrupting the binary IPC stream. The renderer
configures `tracing_subscriber` with `.with_writer(std::io::stderr)` when
`--ipc-stdio` is active.

## Wire Format

Every IPC message is a **length-prefixed JSON frame**:

```text
┌──────────────────────┐──────────────────────────────────────┐
│ Length prefix (4B LE) │ JSON payload (up to 8 MiB)          │
└──────────────────────┘──────────────────────────────────────┘
```

- **Length prefix**: 4 bytes, little-endian `u32`, the byte count of the JSON
  payload (not including the prefix itself).
- **Payload**: A JSON-serialized `IpcMessage` (see below).
- **Maximum frame size**: 8 MiB (`MAX_FRAME_SIZE`). Frames exceeding this
  are rejected with `FrameError::TooLarge`.
- **Zero-length frames**: Rejected with `FrameError::InvalidLength`.

### Framing functions

| Function | Direction | Description |
|----------|-----------|-------------|
| `encode_to_bytes` | Sync | Encode a message to a length-prefixed byte vector |
| `decode_from_bytes` | Sync | Decode a message from a length-prefixed byte slice |
| `write_frame` | Async | Write a message to an `AsyncWrite` sink |
| `read_frame` | Async | Read a message from an `AsyncRead` source |

## Message Types

### IpcMessage (tagged union)

Every IPC frame carries an `IpcMessage`, which is a tagged union that makes
it unambiguous which type of payload is inside:

```rust
#[serde(tag = "direction", content = "payload")]
pub enum IpcMessage {
    #[serde(rename = "core_to_renderer")]
    Command(CommandEnvelope<RendererCommand>),
    #[serde(rename = "renderer_to_core")]
    Event(EventEnvelope<RendererEvent>),
    #[serde(rename = "external_to_core")]
    CoreCommand(CommandEnvelope<CoreCommand>),
    #[serde(rename = "core_broadcast")]
    CoreEvent(EventEnvelope<CoreEvent>),
}
```

### Envelopes

**CommandEnvelope<T>** — wraps a command with a request ID for correlation:

```rust
pub struct CommandEnvelope<T> {
    pub protocol_version: u16,
    pub request_id: RequestId,
    pub payload: T,
}
```

**EventEnvelope<T>** — wraps an event with optional correlation ID:

```rust
pub struct EventEnvelope<T> {
    pub protocol_version: u16,
    pub request_id: Option<RequestId>,
    pub payload: T,
}
```

## Protocol Types

### RendererCommand (Core → Renderer)

| Command | Purpose | Response Event |
|---------|---------|----------------|
| `Start` | Begin rendering | `Ready` |
| `Pause` | Pause (keep resources) | `Paused` |
| `Resume` | Resume from pause | `Resumed` |
| `Stop` | Stop and release resources | `Exited` |
| `ApplyWallpaper(ApplyWallpaperRequest)` | Change wallpaper | `WallpaperApplied` / `WallpaperApplyFailed` |
| `SetMonitor { monitor_id }` | Reassign monitor | `Heartbeat` (ack) |
| `Shutdown` | Graceful shutdown | `Exited` |

### RendererEvent (Renderer → Core)

| Event | Purpose |
|-------|---------|
| `Started { renderer_id }` | Process started |
| `Ready { renderer_id }` | Initialization complete |
| `Heartbeat { renderer_id, uptime_ms }` | Liveness signal |
| `Paused { renderer_id }` | Confirmed pause |
| `Resumed { renderer_id }` | Confirmed resume |
| `WallpaperApplied { renderer_id, monitor_id, wallpaper_id }` | Wallpaper rendering |
| `WallpaperApplyFailed { renderer_id, monitor_id, error: WallpaperApplyError }` | Wallpaper apply failed |
| `Error { renderer_id, message }` | Error occurred |
| `Exited { renderer_id, exit_code }` | Process exiting |

### ApplyWallpaperRequest

```rust
pub struct ApplyWallpaperRequest {
    pub assignment_id: AssignmentId,
    pub monitor_id: MonitorId,
    pub wallpaper_id: WallpaperId,
    pub payload: WallpaperPayload,
}
```

### WallpaperPayload

```rust
#[serde(tag = "type", content = "data")]
pub enum WallpaperPayload {
    #[serde(rename = "static_image")]
    StaticImage(StaticImagePayload),
}
```

### StaticImagePayload

```rust
pub struct StaticImagePayload {
    pub path: PathBuf,
    pub fit_mode: FitMode,
}
```

### FitMode

```rust
pub enum FitMode {
    Fill,
    Fit,
    Stretch,
    Center,
    Tile,
}
```

### WallpaperApplyError

```rust
pub enum WallpaperApplyError {
    FileNotFound,
    DecodeFailed(String),
    UnsupportedFormat(String),
    RenderFailed(String),
}
```

## Protocol Versioning

The current protocol version is **4** (`PROTOCOL_VERSION`).

- **Version 1**: Initial protocol with basic CoreCommand/CoreEvent.
- **Version 2**: Added RendererCommand/RendererEvent with full lifecycle.
- **Version 3**: Added `IpcMessage` tagged union for unambiguous frame decoding.
- **Version 4**: Added `ApplyWallpaperRequest` for structured wallpaper apply commands and `WallpaperApplyFailed` event for error reporting.

Every envelope includes `protocol_version`. Receivers MUST validate the version
and reject mismatched messages with `FrameError::ProtocolVersionMismatch`.

## Error Handling

| Error | Condition |
|-------|-----------|
| `FrameError::TooLarge` | Frame exceeds 8 MiB |
| `FrameError::InvalidLength` | Length prefix is 0 or exceeds max |
| `FrameError::Json` | Payload is not valid JSON |
| `FrameError::ProtocolVersionMismatch` | Version mismatch |
| `FrameError::Io` | I/O error on read/write |

## Future Transport Migration

The stdio transport will eventually be replaced with platform-specific channels:

- **Windows**: Named pipes (`\\.\pipe\wallflow-renderer-{id}`).
- **Linux**: Unix domain sockets.

This migration only requires changing the read/write endpoints, not the
protocol types, framing, or message definitions.
