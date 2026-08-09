use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;

use crate::crypto::CryptoEngine;
use crate::error::{CoreError, CoreResult};
use crate::model::{BatchRecipient, BatchUpload, NotifyPayload};

pub(crate) const MAX_BATCH_ITEMS_PER_UPLOAD: usize = 200;

#[derive(Debug, Default, Clone)]
pub struct BatchBuilder;

impl BatchBuilder {
    #[allow(clippy::too_many_arguments)]
    pub fn build_upload(
        encoded_events: &[Vec<u8>],
        crypto: &CryptoEngine,
        recipients: &[BatchRecipient],
        start_time_ms: i64,
        end_time_ms: i64,
        high_risk_count: u32,
        medium_risk_count: u32,
        notifications: Vec<NotifyPayload>,
    ) -> CoreResult<BatchUpload> {
        if encoded_events.is_empty() {
            return Err(CoreError::InvalidState(
                "cannot build a batch from an empty buffer",
            ));
        }
        if recipients.is_empty() {
            return Err(CoreError::InvalidState(
                "cannot build a batch without any recipients",
            ));
        }

        let msgpack = rmp_serde::to_vec_named(encoded_events)?;

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
            start_time_ms,
            end_time_ms,
            bytes: encrypted,
            access_keys,
            high_risk_count,
            medium_risk_count,
            notifications,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::crypto::encode_batch_event;
    use crate::model::LogEntry;
    use crate::model::UploadKind;

    #[test]
    fn batch_payload_is_array_of_encoded_event_bytes() {
        let entry = LogEntry {
            ts: 123,
            risk: Some(0.5),
            event: UploadKind::Dev {
                title: "test".to_string(),
                details: None,
            },
        };

        let encoded_event = encode_batch_event(&entry).expect("encode event");
        let encoded_batch =
            rmp_serde::to_vec_named(&vec![encoded_event.clone()]).expect("encode batch");
        let decoded_batch: Vec<Vec<u8>> =
            rmp_serde::from_slice(&encoded_batch).expect("decode batch");

        assert_eq!(decoded_batch, vec![encoded_event]);
    }
}
