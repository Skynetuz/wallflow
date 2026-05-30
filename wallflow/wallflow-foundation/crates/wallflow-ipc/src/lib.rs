//! Typed IPC protocol for WallFlow.
//!
//! This crate defines the IPC transport layer used between the Core process
//! and Renderer processes. The wire format is length-prefixed JSON frames
//! over piped stdio (or, in the future, Windows named pipes).
//!
//! ## Wire format
//!
//! ```text
//! [u32 LE length][JSON payload]
//! ```
//!
//! - Length prefix: 4 bytes, little-endian, the byte count of the JSON payload.
//! - Payload: a serialized `IpcMessage` (tagged union with `direction` tag).
//! - Maximum frame size: 8 MiB (`MAX_FRAME_SIZE`).
//!
//! ## Why stdio?
//!
//! Stdio IPC is cloud-testable (works on Linux CI without any platform-specific
//! setup). Logs go to stderr so they don't interfere with the stdout IPC stream.
//! Later, the transport can be swapped to Windows named pipes without changing
//! the protocol types or framing logic.

pub mod frame;
pub mod protocol;

pub use frame::{
    decode_from_bytes, encode_to_bytes, read_frame, validate_protocol_version, write_frame,
    FrameError, LENGTH_PREFIX_SIZE, MAX_FRAME_SIZE,
};
pub use protocol::*;
