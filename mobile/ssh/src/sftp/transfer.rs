//! Bounded request pipelining. JNI batches are larger than individual SFTP packets.
use super::{BATCH, CHUNK, Failure, Result};
use russh_sftp::client::RawSftpSession;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::task::JoinSet;

pub(super) async fn read(
    raw: &Arc<RawSftpSession>,
    handle: &str,
    offset: u64,
    length: usize,
) -> Result<Vec<u8>> {
    let mut tasks = JoinSet::new();
    for start in (0..length).step_by(CHUNK) {
        let (raw, handle) = (raw.clone(), handle.to_owned());
        let count = CHUNK.min(length - start);
        tasks.spawn(async move {
            let mut bytes = Vec::with_capacity(count);
            while bytes.len() < count {
                let part = raw
                    .read(
                        &handle,
                        offset + (start + bytes.len()) as u64,
                        (count - bytes.len()) as u32,
                    )
                    .await?;
                if part.data.is_empty() || part.data.len() > count - bytes.len() {
                    return Err(Failure("SFTP_CHANGED"));
                }
                bytes.extend(part.data);
            }
            Ok::<_, Failure>((start, bytes))
        });
    }
    let mut bytes = vec![0; length];
    let mut failure = None;
    while let Some(result) = tasks.join_next().await {
        match result.map_err(|_| Failure("SFTP_CLOSED")).and_then(|r| r) {
            Ok((start, part)) => bytes[start..start + part.len()].copy_from_slice(&part),
            Err(error) => {
                failure.get_or_insert(error);
            },
        }
    }
    failure.map_or(Ok(bytes), Err)
}

pub(super) async fn write(
    raw: &Arc<RawSftpSession>,
    handle: &str,
    offset: u64,
    bytes: Vec<u8>,
) -> Result<()> {
    let mut tasks = JoinSet::new();
    for (index, part) in bytes.chunks(CHUNK).enumerate() {
        let (raw, handle, part) = (raw.clone(), handle.to_owned(), part.to_vec());
        tasks.spawn(async move {
            raw.write(handle, offset + (index * CHUNK) as u64, part).await.map_err(Failure::from)
        });
    }
    let mut failure = None;
    // Drain every ACK before advancing the contiguous offset or reopening after a failure.
    while let Some(result) = tasks.join_next().await {
        if let Err(error) = result.map_err(|_| Failure("SFTP_CLOSED")).and_then(|r| r) {
            failure.get_or_insert(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

pub(super) async fn verify(
    raw: &Arc<RawSftpSession>,
    handle: &str,
    size: u64,
    expected: &str,
) -> Result<()> {
    let mut digest = Sha256::new();
    let mut offset = 0;
    while offset < size {
        let bytes = read(raw, handle, offset, (size - offset).min(BATCH as u64) as usize).await?;
        offset += bytes.len() as u64;
        digest.update(bytes);
    }
    if format!("{:x}", digest.finalize()) != expected {
        return Err(Failure("SFTP_CHANGED"));
    }
    Ok(())
}
