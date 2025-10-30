use std::marker::PhantomData;

use anyhow::{bail, Result};
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Maximum serialized message size (32 MiB) to guard against malicious clients.
pub const MAX_MESSAGE_SIZE: usize = 32 * 1024 * 1024;

/// Write a length-prefixed bincode encoded message to the provided async writer.
pub async fn write_message<W, T>(writer: &mut W, message: &T) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize + ?Sized,
{
    let payload = serde_json::to_vec(message)?;
    if payload.len() > MAX_MESSAGE_SIZE {
        bail!("message too large: {} bytes", payload.len());
    }

    writer.write_u32_le(payload.len() as u32).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

/// Read a length-prefixed bincode encoded message from the provided async reader.
pub async fn read_message<R, T>(reader: &mut R) -> Result<T>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let len = reader.read_u32_le().await? as usize;
    if len > MAX_MESSAGE_SIZE {
        bail!(
            "message length {} exceeds maximum {}",
            len,
            MAX_MESSAGE_SIZE
        );
    }

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    let msg = serde_json::from_slice::<T>(&buf)?;
    Ok(msg)
}

/// Convenience wrapper that binds a transport type to the shared codec helpers.
pub struct FramedStream<T, Incoming = (), Outgoing = ()> {
    inner: T,
    _marker_in: PhantomData<Incoming>,
    _marker_out: PhantomData<Outgoing>,
}

impl<T, Incoming, Outgoing> FramedStream<T, Incoming, Outgoing>
where
    T: AsyncRead + AsyncWrite + Unpin,
    Incoming: DeserializeOwned,
    Outgoing: Serialize,
{
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            _marker_in: PhantomData,
            _marker_out: PhantomData,
        }
    }

    pub async fn send(&mut self, message: &Outgoing) -> Result<()> {
        write_message(&mut self.inner, message).await
    }

    pub async fn recv(&mut self) -> Result<Incoming> {
        read_message(&mut self.inner).await
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}
