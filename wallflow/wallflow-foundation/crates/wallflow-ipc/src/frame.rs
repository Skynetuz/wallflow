use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Maximum allowed frame size (8 MiB). Frames larger than this are rejected.
pub const MAX_FRAME_SIZE: usize = 8 * 1024 * 1024;

/// Size of the length prefix in bytes (u32 little-endian).
pub const LENGTH_PREFIX_SIZE: usize = 4;

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("frame too large: {actual} bytes, max {max} bytes")]
    TooLarge { actual: usize, max: usize },

    #[error("invalid frame length: {0} bytes (length prefix was zero or exceeds max)")]
    InvalidLength(usize),

    #[error("protocol version mismatch: expected {expected}, got {got}")]
    ProtocolVersionMismatch { expected: u16, got: u16 },
}

/// Writes one length-prefixed JSON frame.
pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_FRAME_SIZE {
        return Err(FrameError::TooLarge {
            actual: bytes.len(),
            max: MAX_FRAME_SIZE,
        });
    }

    let len = bytes.len() as u32;
    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads one length-prefixed JSON frame.
pub async fn read_frame<R, T>(reader: &mut R) -> Result<T, FrameError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len_buf = [0_u8; LENGTH_PREFIX_SIZE];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_SIZE {
        return Err(FrameError::InvalidLength(len));
    }

    let mut payload = vec![0_u8; len];
    reader.read_exact(&mut payload).await?;
    Ok(serde_json::from_slice(&payload)?)
}

/// Encode a serializable value to a length-prefixed byte vector.
/// This is the synchronous equivalent of `write_frame`, suitable for
/// unit tests and non-async contexts.
pub fn encode_to_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, FrameError> {
    let json = serde_json::to_vec(value)?;
    if json.len() > MAX_FRAME_SIZE {
        return Err(FrameError::TooLarge {
            actual: json.len(),
            max: MAX_FRAME_SIZE,
        });
    }
    let len = json.len() as u32;
    let mut buf = Vec::with_capacity(LENGTH_PREFIX_SIZE + json.len());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(&json);
    Ok(buf)
}

/// Decode a deserializable value from a length-prefixed byte slice.
/// This is the synchronous equivalent of `read_frame`, suitable for
/// unit tests and non-async contexts.
pub fn decode_from_bytes<T: DeserializeOwned>(data: &[u8]) -> Result<T, FrameError> {
    if data.len() < LENGTH_PREFIX_SIZE {
        return Err(FrameError::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "not enough bytes for length prefix",
        )));
    }
    let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if len == 0 || len > MAX_FRAME_SIZE {
        return Err(FrameError::InvalidLength(len));
    }
    let payload = data
        .get(LENGTH_PREFIX_SIZE..LENGTH_PREFIX_SIZE + len)
        .ok_or_else(|| {
            FrameError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "expected {len} bytes of payload, got {}",
                    data.len() - LENGTH_PREFIX_SIZE
                ),
            ))
        })?;
    Ok(serde_json::from_slice(payload)?)
}

/// Validate the protocol version of an incoming envelope.
/// Returns an error if the version does not match the expected version.
pub fn validate_protocol_version(envelope_version: u16, expected: u16) -> Result<(), FrameError> {
    if envelope_version != expected {
        return Err(FrameError::ProtocolVersionMismatch {
            expected,
            got: envelope_version,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Envelope, PROTOCOL_VERSION};

    #[tokio::test]
    async fn roundtrips_json_frame() {
        let (mut client, mut server) = tokio::io::duplex(4096);
        let msg = Envelope::event(crate::RendererCommand::Start);
        write_frame(&mut client, &msg).await.unwrap();
        let decoded: Envelope<crate::RendererCommand> = read_frame(&mut server).await.unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn encode_decode_sync_roundtrip() {
        let msg = Envelope::event(crate::RendererCommand::Pause);
        let bytes = encode_to_bytes(&msg).unwrap();
        let decoded: Envelope<crate::RendererCommand> = decode_from_bytes(&bytes).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn encode_decode_renderer_event() {
        let msg = Envelope::event(crate::RendererEvent::Heartbeat {
            renderer_id: wallflow_common::RendererId::new(),
            uptime_ms: 1234,
        });
        let bytes = encode_to_bytes(&msg).unwrap();
        let decoded: Envelope<crate::RendererEvent> = decode_from_bytes(&bytes).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn reject_too_large_frame() {
        // Create a vector that exceeds MAX_FRAME_SIZE
        let big = vec![0u8; MAX_FRAME_SIZE + 1];
        let _data: Vec<u8> = vec![];
        // Try to encode — the JSON of the data itself is small, so test the length check directly
        let len = (MAX_FRAME_SIZE + 1) as u32;
        let mut buf = len.to_le_bytes().to_vec();
        buf.extend_from_slice(&big);
        let result: Result<serde_json::Value, _> = decode_from_bytes(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn reject_zero_length_frame() {
        let buf = 0u32.to_le_bytes().to_vec();
        let result: Result<serde_json::Value, _> = decode_from_bytes(&buf);
        assert!(result.is_err());
        match result {
            Err(FrameError::InvalidLength(0)) => {}
            other => panic!("expected InvalidLength(0), got {other:?}"),
        }
    }

    #[test]
    fn reject_invalid_json() {
        let bad_json = b"not valid json at all";
        let len = bad_json.len() as u32;
        let mut buf = len.to_le_bytes().to_vec();
        buf.extend_from_slice(bad_json);
        let result: Result<serde_json::Value, _> = decode_from_bytes(&buf);
        assert!(result.is_err());
        match result {
            Err(FrameError::Json(_)) => {}
            other => panic!("expected Json error, got {other:?}"),
        }
    }

    #[test]
    fn reject_insufficient_bytes() {
        let buf = [1u8, 2, 3]; // less than 4 bytes for length prefix
        let result: Result<serde_json::Value, _> = decode_from_bytes(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn protocol_version_mismatch() {
        let result = validate_protocol_version(1, PROTOCOL_VERSION);
        assert!(result.is_err());
        match result {
            Err(FrameError::ProtocolVersionMismatch { expected, got }) => {
                assert_eq!(expected, PROTOCOL_VERSION);
                assert_eq!(got, 1);
            }
            other => panic!("expected ProtocolVersionMismatch, got {other:?}"),
        }
    }

    #[test]
    fn protocol_version_match_ok() {
        assert!(validate_protocol_version(PROTOCOL_VERSION, PROTOCOL_VERSION).is_ok());
    }

    #[tokio::test]
    async fn multiple_frames_on_same_stream() {
        let (mut client, mut server) = tokio::io::duplex(8192);
        let msg1 = Envelope::event(crate::RendererCommand::Start);
        let msg2 = Envelope::event(crate::RendererCommand::Pause);
        let msg3 = Envelope::event(crate::RendererCommand::Shutdown);

        write_frame(&mut client, &msg1).await.unwrap();
        write_frame(&mut client, &msg2).await.unwrap();
        write_frame(&mut client, &msg3).await.unwrap();

        let decoded1: Envelope<crate::RendererCommand> = read_frame(&mut server).await.unwrap();
        let decoded2: Envelope<crate::RendererCommand> = read_frame(&mut server).await.unwrap();
        let decoded3: Envelope<crate::RendererCommand> = read_frame(&mut server).await.unwrap();

        assert_eq!(msg1, decoded1);
        assert_eq!(msg2, decoded2);
        assert_eq!(msg3, decoded3);
    }
}
