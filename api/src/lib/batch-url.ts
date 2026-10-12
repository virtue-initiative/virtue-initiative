import { Env } from '../types/bindings';

type StoredBatch = { id: string; user_id: string; version: string };

/**
 * API-038: a batch's object storage key is not stored. It is derived from the
 * batch's version, owner and id. A change to the storage layout MUST add a case
 * for the versions that use the new layout and leave this one as it is, so
 * batches uploaded under an earlier version keep resolving to where they were
 * stored.
 */
export function batchObjectKey(batch: StoredBatch): string {
  switch (batch.version) {
    default:
      return `user/${batch.user_id}/batches/${batch.id}.enc`;
  }
}

export function batchUrl(env: Env, batch: StoredBatch): string {
  return `${env.R2_URL}/${batchObjectKey(batch)}`;
}
