use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::warn;
use uuid::Uuid;

use shared::prelude::*;

pub struct FileDownloadManager {
    root: PathBuf,
    default_auto_open: bool,
    sessions: Arc<Mutex<HashMap<Uuid, DownloadSession>>>,
}

impl FileDownloadManager {
    pub fn new(root: PathBuf, default_auto_open: bool) -> Self {
        Self {
            root,
            default_auto_open,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn handle_offer(&self, offer: &FileOffer) -> Result<PathBuf> {
        let sanitized = sanitize_filename(&offer.file_name);
        let target = self.root.join(&sanitized);

        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("无法创建目录 {}", parent.display()))?;
        }

        let file = File::create(&target)
            .await
            .with_context(|| format!("无法创建文件 {}", target.display()))?;

        let mut sessions = self.sessions.lock();
        sessions.insert(
            offer.transfer_id,
            DownloadSession {
                file,
                path: target.clone(),
                expected: offer.total_size,
                received: 0,
                auto_open: offer.auto_open || self.default_auto_open,
            },
        );

        Ok(target)
    }

    pub async fn handle_chunk(&self, chunk: &FileChunk) -> Result<()> {
        let mut sessions = self.sessions.lock();
        if let Some(session) = sessions.get_mut(&chunk.transfer_id) {
            session
                .file
                .write_all(&chunk.bytes)
                .await
                .with_context(|| format!("写入文件 {} 失败", session.path.display()))?;
            session.received += chunk.bytes.len() as u64;
        } else {
            warn!(transfer = %chunk.transfer_id, "收到未知的文件分片");
        }
        Ok(())
    }

    pub async fn handle_complete(
        &self,
        complete: &FileTransferComplete,
    ) -> Result<Option<PathBuf>> {
        let mut sessions = self.sessions.lock();
        if let Some(mut session) = sessions.remove(&complete.transfer_id) {
            session.file.flush().await?;
            if !complete.success {
                warn!("文件传输失败: {:?}", complete.message);
                return Ok(None);
            }

            if session.received != session.expected {
                warn!(
                    expected = session.expected,
                    received = session.received,
                    "文件大小与期望不符"
                );
            }

            if session.auto_open {
                return Ok(Some(session.path));
            }
        } else {
            warn!(transfer = %complete.transfer_id, "收到未知的完成通知");
        }

        Ok(None)
    }
}

struct DownloadSession {
    file: File,
    path: PathBuf,
    expected: u64,
    received: u64,
    auto_open: bool,
}
