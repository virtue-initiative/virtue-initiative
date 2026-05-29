use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;

use crate::crypto::{CryptoEngine, encode_batch_event};
use crate::error::{CoreError, CoreResult};
use crate::model::{BatchLogEntry, BatchRecipient, BatchUpload};

#[derive(Debug, Default, Clone)]
pub struct BatchBuilder;

impl BatchBuilder {
    pub fn build_upload(
        items: &[BatchLogEntry],
        crypto: &CryptoEngine,
        recipients: &[BatchRecipient],
        end_time_ms: i64,
    ) -> CoreResult<BatchUpload> {
        let first = items.first().ok_or(CoreError::InvalidState(
            "cannot build a batch from an empty buffer",
        ))?;
        if recipients.is_empty() {
            return Err(CoreError::InvalidState(
                "cannot build a batch without any recipients",
            ));
        }

        let encoded_events = items
            .iter()
            .map(|item| encode_batch_event(&item.event))
            .collect::<CoreResult<Vec<_>>>()?;
        let msgpack = rmp_serde::to_vec_named(&encoded_events)?;

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
}

#[cfg(test)]
mod tests {
    use crate::crypto::encode_batch_event;
    use crate::model::{EventData, LogEntry};

    #[test]
    fn batch_payload_is_array_of_encoded_event_bytes() {
        let event = LogEntry {
            ts: 123,
            kind: "developer_log".to_string(),
            risk: Some(0.5),
            data: EventData::from_pairs([("source".to_string(), "test".to_string())]),
        };

        let encoded_event = encode_batch_event(&event).expect("encode event");
        let encoded_batch =
            rmp_serde::to_vec_named(&vec![encoded_event.clone()]).expect("encode batch");
        let decoded_batch: Vec<Vec<u8>> =
            rmp_serde::from_slice(&encoded_batch).expect("decode batch");

        assert_eq!(decoded_batch, vec![encoded_event]);
    }
}
