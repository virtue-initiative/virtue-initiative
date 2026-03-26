use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Serialize;

use crate::crypto::CryptoEngine;
use crate::error::{CoreError, CoreResult};
use crate::model::{BatchBufferState, BatchEvent, BatchRecipient, BatchUpload, BufferedScreenshot};

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
        recipients: &[BatchRecipient],
        end_time_ms: i64,
    ) -> CoreResult<BatchUpload> {
        let first = buffer.screenshots.first().ok_or(CoreError::InvalidState(
            "cannot build a batch from an empty buffer",
        ))?;
        if recipients.is_empty() {
            return Err(CoreError::InvalidState(
                "cannot build a batch without any recipients",
            ));
        }

        let events: Vec<&BatchEvent> = buffer.screenshots.iter().map(|item| &item.event).collect();
        let msgpack = rmp_serde::to_vec_named(&BatchEnvelope { events })?;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&msgpack)?;
        let gzipped = encoder.finish()?;
        let batch_key = crypto.generate_batch_key();
        let encrypted = crypto.encrypt_batch_blob(&batch_key, &gzipped)?;
        let access_keys = recipients
            .iter()
            .map(|recipient| {
                Ok(crate::model::BatchAccessKey {
                    user_id: recipient.user_id.clone(),
                    hpke_key_base64: crypto.wrap_batch_key_for_recipient(recipient, &batch_key)?,
                })
            })
            .collect::<CoreResult<Vec<_>>>()?;

        Ok(BatchUpload {
            start_time_ms: first.event.ts,
            end_time_ms,
            bytes: encrypted,
            access_keys,
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
