import {
  deleteExpiredBatchesForUsers,
  deleteExpiredDeviceSessionsChunk,
  deleteExpiredEmailTokensChunk,
  deleteExpiredLockedPasswordsChunk,
  deleteExpiredUserSessionsChunk,
} from './db';
import { Env } from '../types/bindings';

const RETENTION_MS = 30 * 24 * 60 * 60 * 1000;
// api/SPEC.md API-047: soft-deleted locked passwords are hard-deleted after 7 days.
const LOCKED_PASSWORD_RETENTION_MS = 7 * 24 * 60 * 60 * 1000;
const PRUNE_CHUNK_LIMIT = 500; // cap per DB round-trip so one cron tick can't stall
// Users with batches whose expired ones one round-trip deletes. A day of uploads is about
// 16 batches per user, so a chunk deletes some 8,000 rows. A chunk is two of the 1,000
// queries D1 allows one Worker invocation, so one daily run covers up to about 250,000
// users who uploaded in the last 30 days. Users who haven't cost nothing.
const PRUNE_USERS_PER_CHUNK = 500;

// R2 objects for expired batches are pruned separately by an R2 bucket lifecycle
// rule (30-day expiration on the bucket that only ever holds batch blobs), not
// here -- see the plan/PR notes for the one-time `wrangler r2 bucket lifecycle
// add` setup this depends on.
//
// Runs once a day, so a batch row can outlive the 30 days by up to a day, after its
// R2 object is gone. The web cache never downloads a batch past 30 days, so nothing
// reads it in that time.
export async function pruneExpiredBatches(
  env: Env,
  now = Date.now(),
  usersPerChunk = PRUNE_USERS_PER_CHUNK,
) {
  const cutoff = now - RETENTION_MS;
  let deletedTotal = 0;
  let afterUserId: Uint8Array | null = new Uint8Array(0); // sorts before every id

  while (afterUserId) {
    const chunk = await deleteExpiredBatchesForUsers(env.DB, afterUserId, usersPerChunk, cutoff);
    deletedTotal += chunk.deleted;
    afterUserId = chunk.nextUserId;
  }

  return deletedTotal;
}

export async function pruneExpiredEmailTokens(env: Env, now = Date.now()) {
  let deletedTotal = 0;

  while (true) {
    const deleted = await deleteExpiredEmailTokensChunk(env.DB, now, PRUNE_CHUNK_LIMIT);
    deletedTotal += deleted;
    if (deleted < PRUNE_CHUNK_LIMIT) break; // drained
  }

  return deletedTotal;
}

export async function pruneExpiredUserSessions(env: Env, now = Date.now()) {
  let deletedTotal = 0;

  while (true) {
    const deleted = await deleteExpiredUserSessionsChunk(env.DB, now, PRUNE_CHUNK_LIMIT);
    deletedTotal += deleted;
    if (deleted < PRUNE_CHUNK_LIMIT) break; // drained
  }

  return deletedTotal;
}

export async function pruneExpiredDeviceSessions(env: Env, now = Date.now()) {
  let deletedTotal = 0;

  while (true) {
    const deleted = await deleteExpiredDeviceSessionsChunk(env.DB, now, PRUNE_CHUNK_LIMIT);
    deletedTotal += deleted;
    if (deleted < PRUNE_CHUNK_LIMIT) break; // drained
  }

  return deletedTotal;
}

export async function pruneExpiredLockedPasswords(env: Env, now = Date.now()) {
  const cutoff = now - LOCKED_PASSWORD_RETENTION_MS;
  let deletedTotal = 0;

  while (true) {
    const deleted = await deleteExpiredLockedPasswordsChunk(env.DB, cutoff, PRUNE_CHUNK_LIMIT);
    deletedTotal += deleted;
    if (deleted < PRUNE_CHUNK_LIMIT) break; // drained
  }

  return deletedTotal;
}
