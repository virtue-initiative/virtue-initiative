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
                model_runs: None,
                images: None,
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

    // BATCH-002, BATCH-004, BATCH-005, BATCH-006: the production BatchBuilder
    // turns each vector's events into a batch that a consumer holding the
    // vector's recipient private key opens to exactly those encoded events.
    #[test]
    fn build_upload_output_opens_for_every_vector_case() {
        use aes_gcm::aead::{Aead, KeyInit};
        use aes_gcm::{Aes256Gcm, Nonce};
        use base64::Engine;
        use hpke::aead::AesGcm256;
        use hpke::kdf::HkdfSha256;
        use hpke::kem::{Kem as KemTrait, X25519HkdfSha256};
        use hpke::{Deserializable, OpModeR, setup_receiver};
        use std::io::Read;

        use super::BatchBuilder;
        use crate::crypto::CryptoEngine;
        use crate::model::BatchRecipient;

        fn unhex(s: &str) -> Vec<u8> {
            (0..s.len() / 2)
                .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
                .collect()
        }

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../batch-format/vectors/batch.json");
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("read batch.json"))
                .expect("parse batch.json");
        let mut built = 0;
        for case in file["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let encoded: Vec<Vec<u8>> = case["encoded_events_hex"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| unhex(e.as_str().unwrap()))
                .collect();
            let recipient = BatchRecipient {
                user_id: "vector".to_string(),
                pub_key_base64: case["recipient_public_key_base64"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            };
            let upload = BatchBuilder::build_upload(
                &encoded,
                &CryptoEngine,
                std::slice::from_ref(&recipient),
                0,
                0,
                0,
                0,
                0,
                Vec::new(),
            );
            if encoded.is_empty() {
                // BATCH-002: a batch MUST contain at least one event.
                assert!(upload.is_err(), "{name}: empty batch must be refused");
                continue;
            }
            let upload = upload.unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert_eq!(upload.total_count as usize, encoded.len(), "{name}");

            let sk = unhex(case["recipient_private_key_hex"].as_str().unwrap());
            let sk =
                <<X25519HkdfSha256 as KemTrait>::PrivateKey as Deserializable>::from_bytes(&sk)
                    .unwrap();
            let envelope = base64::engine::general_purpose::STANDARD
                .decode(&upload.access_keys[0].hpke_key_base64)
                .unwrap();
            let enc = <<X25519HkdfSha256 as KemTrait>::EncappedKey as Deserializable>::from_bytes(
                &envelope[..32],
            )
            .unwrap();
            let mut ctx = setup_receiver::<AesGcm256, HkdfSha256, X25519HkdfSha256>(
                &OpModeR::Base,
                &sk,
                &enc,
                b"",
            )
            .unwrap();
            let batch_key = ctx.open(&envelope[32..], b"").unwrap();

            let cipher = Aes256Gcm::new_from_slice(&batch_key).unwrap();
            let gz = cipher
                .decrypt(Nonce::from_slice(&upload.bytes[..12]), &upload.bytes[12..])
                .unwrap();
            let mut payload = Vec::new();
            flate2::read::GzDecoder::new(&gz[..])
                .read_to_end(&mut payload)
                .unwrap();
            let decoded: Vec<serde_bytes::ByteBuf> = rmp_serde::from_slice(&payload).unwrap();
            let decoded: Vec<Vec<u8>> = decoded.into_iter().map(|b| b.into_vec()).collect();
            assert_eq!(decoded, encoded, "{name}");
            built += 1;
        }
        assert!(built >= 5, "expected several non-empty batch cases");

        // BATCH-005: a batch MUST NOT be built without recipients.
        let none = BatchBuilder::build_upload(
            &[vec![0x80]],
            &CryptoEngine,
            &[],
            0,
            0,
            0,
            0,
            0,
            Vec::new(),
        );
        assert!(none.is_err());
    }
}
