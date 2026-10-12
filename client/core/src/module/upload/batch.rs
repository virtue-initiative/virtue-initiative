use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde_bytes::Bytes;

use crate::crypto::CryptoEngine;
use crate::error::{CoreError, CoreResult};
use crate::model::{BatchRecipient, BatchUpload, NotifyPayload};

pub(crate) const MAX_BATCH_ITEMS_PER_UPLOAD: usize = 200;

/// The msgpack payload: an array holding each encoded event as a `bin`
/// value (BATCH-002, BATCH-004).
fn encode_payload(encoded_events: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let events: Vec<&Bytes> = encoded_events.iter().map(|e| Bytes::new(e)).collect();
    Ok(rmp_serde::to_vec_named(&events)?)
}

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
        screenshot_count: u32,
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

        let msgpack = encode_payload(encoded_events)?;

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
            total_count: encoded_events.len() as u32,
            high_risk_count,
            medium_risk_count,
            screenshot_count,
            notifications,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_bytes::ByteBuf;

    use super::encode_payload;
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
            encode_payload(std::slice::from_ref(&encoded_event)).expect("encode batch");
        let decoded_batch: Vec<ByteBuf> =
            rmp_serde::from_slice(&encoded_batch).expect("decode batch");

        assert_eq!(decoded_batch, vec![ByteBuf::from(encoded_event)]);
    }

    /// BATCH-004: byte strings are msgpack `bin`, not arrays of integers.
    #[test]
    fn byte_strings_are_encoded_as_msgpack_bin() {
        let image = vec![0_u8, 127, 128, 255];
        let entry = LogEntry {
            ts: 123,
            risk: None,
            event: UploadKind::Screenshot {
                image: image.clone(),
                content_type: "image/webp".to_string(),
                skin_detection: None,
                nsfw_detection: None,
            },
        };

        let encoded_event = encode_batch_event(&entry).expect("encode event");
        // bin8 marker, length, then the raw bytes.
        let image_bin = [&[0xc4, image.len() as u8][..], &image].concat();
        assert!(
            encoded_event
                .windows(image_bin.len())
                .any(|window| window == image_bin),
            "image must be a bin value"
        );
        let decoded: LogEntry = rmp_serde::from_slice(&encoded_event).expect("decode event");
        assert!(matches!(decoded.event, UploadKind::Screenshot { image: got, .. } if got == image));

        // State files written before this change hold the image as a JSON
        // array of numbers, and must still load.
        let legacy: LogEntry = serde_json::from_value(serde_json::json!({
            "ts": 123,
            "type": "screenshot",
            "data": { "image": image, "content_type": "image/webp" }
        }))
        .expect("decode legacy state");
        assert!(matches!(legacy.event, UploadKind::Screenshot { image: got, .. } if got == image));

        let payload = encode_payload(std::slice::from_ref(&encoded_event)).expect("encode batch");
        // fixarray of one element, then a bin8 holding the event.
        assert_eq!(payload[..3], [0x91, 0xc4, encoded_event.len() as u8]);
        assert_eq!(payload[3..], encoded_event[..]);
    }
}
