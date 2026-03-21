use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Serialize;

use crate::crypto::CryptoEngine;
use crate::error::{CoreError, CoreResult};
use crate::model::{BatchBufferState, BatchEvent, BatchUpload, BufferedScreenshot};

#[derive(Debug, Default, Clone)]
pub struct BatchBuilder;

impl BatchBuilder {
    pub fn push_screenshot(
        buffer: &mut BatchBufferState,
        screenshot: BufferedScreenshot,
    ) -> CoreResult<()> {
        buffer.screenshots.push(screenshot);
        buffer.screenshots.sort_by_key(|item| item.event.ts);
        Ok(())
    }

    pub fn build_upload(
        buffer: &BatchBufferState,
        crypto: &CryptoEngine,
        end_time_ms: i64,
    ) -> CoreResult<BatchUpload> {
        let first = buffer.screenshots.first().ok_or(CoreError::InvalidState(
            "cannot build a batch from an empty buffer",
        ))?;
        let events: Vec<&BatchEvent> = buffer.screenshots.iter().map(|item| &item.event).collect();
        let msgpack = rmp_serde::to_vec_named(&BatchEnvelope { events })?;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&msgpack)?;
        let gzipped = encoder.finish()?;
        let encrypted = crypto.encrypt_batch_blob(&gzipped)?;

        Ok(BatchUpload {
            start_time_ms: first.event.ts,
            end_time_ms,
            bytes: encrypted,
        })
    }

    pub fn clear(buffer: &mut BatchBufferState) {
        buffer.screenshots.clear();
    }
}

#[derive(Serialize)]
struct BatchEnvelope<'a> {
    events: Vec<&'a BatchEvent>,
}
