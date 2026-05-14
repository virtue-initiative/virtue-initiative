use std::collections::{HashMap, HashSet};

use rand_core::{OsRng, TryRngCore};

use crate::model::{AuditLogItem, AuditRecord, AuditState, StoredAuditRecord};

pub fn generate_local_id() -> String {
    let mut bytes = [0_u8; 16];
    let mut rng = OsRng;
    rng.try_fill_bytes(&mut bytes)
        .expect("OS RNG unavailable for local id generation");
    hex::encode(bytes)
}

pub fn derive_state(records: &[StoredAuditRecord]) -> AuditState {
    // Pass 1: collect upload markers so we know which Log records are already done.
    let mut hash_uploaded = HashSet::<String>::new();
    let mut log_uploaded = HashSet::<String>::new();
    for record in records {
        match &record.record {
            AuditRecord::HashUploaded { local_id } => {
                hash_uploaded.insert(local_id.clone());
            }
            AuditRecord::LogUploaded { local_id, .. } => {
                log_uploaded.insert(local_id.clone());
            }
            _ => {}
        }
    }

    // Pass 2: build items only for logs that haven't been uploaded yet.
    // Skipping uploaded records avoids holding their (potentially large) payloads in memory;
    // the data stays on disk in the JSONL files and is still tracked via log_uploaded above.
    let mut by_id = HashMap::<String, AuditLogItem>::new();
    let mut order = Vec::<String>::new();
    for record in records {
        if let AuditRecord::Log {
            local_id,
            should_be_in_batch,
            requires_hash_upload,
            log,
        } = &record.record
        {
            if log_uploaded.contains(local_id) {
                continue;
            }
            if by_id.contains_key(local_id) {
                continue;
            }
            by_id.insert(
                local_id.clone(),
                AuditLogItem {
                    audit_day: record.audit_day.clone(),
                    local_id: local_id.clone(),
                    should_be_in_batch: *should_be_in_batch,
                    requires_hash_upload: *requires_hash_upload,
                    payload: log.clone(),
                },
            );
            order.push(local_id.clone());
        }
    }

    let items = order
        .iter()
        .filter_map(|local_id| by_id.get(local_id).cloned())
        .collect::<Vec<_>>();
    let pending_hash_uploads = items
        .iter()
        .filter(|item| item.requires_hash_upload && !hash_uploaded.contains(&item.local_id))
        .cloned()
        .collect::<Vec<_>>();
    let pending_direct_uploads = items
        .iter()
        .filter(|item| !item.should_be_in_batch)
        .cloned()
        .collect::<Vec<_>>();
    let pending_batch_uploads = items
        .iter()
        .filter(|item| {
            item.should_be_in_batch
                && (!item.requires_hash_upload || hash_uploaded.contains(&item.local_id))
        })
        .cloned()
        .collect::<Vec<_>>();
    let pending_request_count = pending_hash_uploads.len()
        + pending_direct_uploads.len()
        + usize::from(!pending_batch_uploads.is_empty());

    AuditState {
        items,
        pending_hash_uploads,
        pending_direct_uploads,
        pending_batch_uploads,
        pending_request_count,
    }
}
