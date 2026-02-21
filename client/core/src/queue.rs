use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BufferedUpload {
    pub id: String,
    pub device_id: String,
    pub taken_at: DateTime<Utc>,
    pub content_type: String,
    pub payload: Vec<u8>,
    pub sha256_hex: String,
    pub attempts: u32,
    pub next_attempt_at: DateTime<Utc>,
}

impl BufferedUpload {
    pub fn new(
        id: impl Into<String>,
        device_id: impl Into<String>,
        taken_at: DateTime<Utc>,
        content_type: impl Into<String>,
        payload: Vec<u8>,
        sha256_hex: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            device_id: device_id.into(),
            taken_at,
            content_type: content_type.into(),
            payload,
            sha256_hex: sha256_hex.into(),
            attempts: 0,
            next_attempt_at: Utc::now(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct QueueEnqueueResult {
    pub len: usize,
    pub dropped_oldest: bool,
}

#[derive(Debug)]
pub struct PersistentQueue {
    path: PathBuf,
    max_items: usize,
    items: Mutex<VecDeque<BufferedUpload>>,
}

impl PersistentQueue {
    pub fn open(path: impl AsRef<Path>, max_items: usize) -> CoreResult<Self> {
        let path = path.as_ref().to_path_buf();
        let items = load_queue(&path)?;

        Ok(Self {
            path,
            max_items,
            items: Mutex::new(items),
        })
    }

    pub fn enqueue(&self, item: BufferedUpload) -> CoreResult<QueueEnqueueResult> {
        let mut guard = self
            .items
            .lock()
            .map_err(|_| CoreError::QueueLockPoisoned)?;

        let dropped_oldest = if guard.len() >= self.max_items {
            guard.pop_front();
            true
        } else {
            false
        };

        guard.push_back(item);
        persist_queue(&self.path, &guard)?;

        Ok(QueueEnqueueResult {
            len: guard.len(),
            dropped_oldest,
        })
    }

    pub fn peek_front(&self) -> CoreResult<Option<BufferedUpload>> {
        let guard = self
            .items
            .lock()
            .map_err(|_| CoreError::QueueLockPoisoned)?;
        Ok(guard.front().cloned())
    }

    pub fn pop_front(&self) -> CoreResult<Option<BufferedUpload>> {
        let mut guard = self
            .items
            .lock()
            .map_err(|_| CoreError::QueueLockPoisoned)?;
        let popped = guard.pop_front();
        persist_queue(&self.path, &guard)?;
        Ok(popped)
    }

    pub fn mark_front_retry(
        &self,
        next_attempt_at: DateTime<Utc>,
    ) -> CoreResult<Option<BufferedUpload>> {
        let mut guard = self
            .items
            .lock()
            .map_err(|_| CoreError::QueueLockPoisoned)?;

        let item = guard.front_mut().map(|item| {
            item.attempts = item.attempts.saturating_add(1);
            item.next_attempt_at = next_attempt_at;
            item.clone()
        });

        persist_queue(&self.path, &guard)?;
        Ok(item)
    }

    pub fn front_is_ready(&self, now: DateTime<Utc>) -> CoreResult<bool> {
        let guard = self
            .items
            .lock()
            .map_err(|_| CoreError::QueueLockPoisoned)?;
        Ok(match guard.front() {
            Some(item) => item.next_attempt_at <= now,
            None => false,
        })
    }

    pub fn len(&self) -> CoreResult<usize> {
        let guard = self
            .items
            .lock()
            .map_err(|_| CoreError::QueueLockPoisoned)?;
        Ok(guard.len())
    }

    pub fn is_empty(&self) -> CoreResult<bool> {
        Ok(self.len()? == 0)
    }
}

fn load_queue(path: &Path) -> CoreResult<VecDeque<BufferedUpload>> {
    if !path.exists() {
        return Ok(VecDeque::new());
    }

    let bytes = fs::read(path)?;
    if bytes.is_empty() {
        return Ok(VecDeque::new());
    }

    Ok(serde_json::from_slice(&bytes)?)
}

fn persist_queue(path: &Path, queue: &VecDeque<BufferedUpload>) -> CoreResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension("tmp");
    let bytes = serde_json::to_vec(queue)?;
    fs::write(&tmp_path, bytes)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tempfile::tempdir;

    use super::{BufferedUpload, PersistentQueue};

    #[test]
    fn queue_persists_to_disk() {
        let dir = tempdir().expect("tempdir");
        let queue_path = dir.path().join("uploads.json");

        let queue = PersistentQueue::open(&queue_path, 10).expect("open queue");
        let item = BufferedUpload::new(
            "item-1",
            "device-1",
            Utc::now(),
            "image/jpeg",
            vec![1, 2, 3],
            "deadbeef",
        );
        queue.enqueue(item).expect("enqueue");

        let queue_reloaded = PersistentQueue::open(&queue_path, 10).expect("reload queue");
        assert_eq!(queue_reloaded.len().expect("len"), 1);
    }

    #[test]
    fn queue_drops_oldest_when_full() {
        let dir = tempdir().expect("tempdir");
        let queue_path = dir.path().join("uploads.json");

        let queue = PersistentQueue::open(&queue_path, 1).expect("open queue");

        let first = BufferedUpload::new(
            "item-1",
            "device-1",
            Utc::now(),
            "image/jpeg",
            vec![1],
            "aaa",
        );
        let second = BufferedUpload::new(
            "item-2",
            "device-1",
            Utc::now(),
            "image/jpeg",
            vec![2],
            "bbb",
        );

        queue.enqueue(first).expect("enqueue first");
        let result = queue.enqueue(second).expect("enqueue second");

        assert!(result.dropped_oldest);
        let front = queue.peek_front().expect("peek").expect("front item");
        assert_eq!(front.id, "item-2");
    }
}
