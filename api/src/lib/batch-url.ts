import { Env } from '../types/bindings';

type StoredBatch = { id: string; user_id: string; version: string };

/**
 * API-038: where a batch lives is derived from its version, owner and id, never
 * stored. `key` names the object in the R2 bucket and `url` is where clients
 * download it. A move to new storage MUST add a case for the versions stored
 * there and leave this one as it is, so batches uploaded under an earlier
 * version keep resolving to where they were stored.
 */
export function batchObject(env: Pick<Env, 'R2_URL'>, batch: StoredBatch) {
  switch (batch.version) {
    default: {
      const key = `user/${batch.user_id}/batches/${batch.id}.enc`;
      return { key, url: `${env.R2_URL}/${key}` };
    }
  }
}
