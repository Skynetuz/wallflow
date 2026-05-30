//! Typed IPC protocol for WallFlow.

pub mod frame;
pub mod protocol;

pub use frame::{read_frame, write_frame, FrameError, MAX_FRAME_SIZE};
pub use protocol::*;
