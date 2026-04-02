use std::collections::{HashMap, HashSet};

use rand_core::{OsRng, TryRngCore};

use crate::model::{AuditLogItem, AuditRecord, AuditState};

pub fn generate_local_id() -> String {
    let mut bytes = [0_u8; 16];
    let mut rng = OsRng;
    rng.try_fill_bytes(&mut bytes)
        .expect("OS RNG unavailable for local id generation");
    hex::encode(bytes)
}

pub fn derive_state(records: &[AuditRecord]) -> AuditState {
    let mut by_id = HashMap::<String, AuditLogItem>::new();
    let mut order = Vec::<String>::new();
    let mut hash_uploaded = HashSet::<String>::new();
    let mut log_uploaded = HashSet::<String>::new();

    for record in records {
        match record {
            AuditRecord::Log {
                local_id,
                should_be_in_batch,
                requires_hash_upload,
                log,
            } => {
                if by_id.contains_key(local_id) {
                    continue;
                }
                by_id.insert(
                    local_id.clone(),
                    AuditLogItem {
                        local_id: local_id.clone(),
                        should_be_in_batch: *should_be_in_batch,
                        requires_hash_upload: *requires_hash_upload,
                        payload: log.clone(),
                    },
                );
                order.push(local_id.clone());
            }
            AuditRecord::HashUploaded { local_id } => {
                hash_uploaded.insert(local_id.clone());
            }
            AuditRecord::LogUploaded { local_id, .. } => {
                log_uploaded.insert(local_id.clone());
            }
            AuditRecord::BatchUploaded { .. } => {}
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
        .filter(|item| !item.should_be_in_batch && !log_uploaded.contains(&item.local_id))
        .cloned()
        .collect::<Vec<_>>();
    let pending_batch_uploads = items
        .iter()
        .filter(|item| {
            item.should_be_in_batch
                && !log_uploaded.contains(&item.local_id)
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
