use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_FRAME_SIZE: usize = 8 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("frame too large: {actual} bytes, max {max} bytes")]
    TooLarge { actual: usize, max: usize },
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
    let mut len_buf = [0_u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_SIZE {
        return Err(FrameError::TooLarge {
            actual: len,
            max: MAX_FRAME_SIZE,
        });
    }

    let mut payload = vec![0_u8; len];
    reader.read_exact(&mut payload).await?;
    Ok(serde_json::from_slice(&payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoreCommand, Envelope, RequestId};

    #[tokio::test]
    async fn roundtrips_json_frame() {
        let (mut client, mut server) = tokio::io::duplex(4096);
        let msg = Envelope::request(RequestId::new(), CoreCommand::GetMonitors);
        write_frame(&mut client, &msg).await.unwrap();
        let decoded: Envelope<CoreCommand> = read_frame(&mut server).await.unwrap();
        assert_eq!(msg, decoded);
    }
}
