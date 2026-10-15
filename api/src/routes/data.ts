import { Hono } from 'hono';
import { z } from 'zod';
import { authenticateWebSession } from '../middleware/auth';
import {
  findUserById,
  listBatches,
  listIncomingPartners,
  listOwnedPartners,
  listVisibleOwnerIds,
} from '../lib/db';
import { wrappedKeyForUser } from '../lib/access-keys';
import { batchObject } from '../lib/batch-url';
import { encodeHex } from '../lib/encoding';
import { serializeUser, serializeWatchers, serializeWatching } from '../lib/serializers';
import { validateZ } from '../middleware/validation';
import { Env, Variables } from '../types/bindings';

const data = new Hono<{ Bindings: Env; Variables: Variables }>();

// batches.end_hash is a 32-byte BLOB, which D1 hands back as an ArrayBuffer or a
// plain array of byte values.
function endHashHex(stored: unknown): string {
  if (typeof stored === 'string') return stored;
  return encodeHex(stored instanceof ArrayBuffer ? stored : Uint8Array.from(stored as number[]));
}

const listDataSchema = z.object({
  since: z.coerce.number().int().nonnegative().optional().default(0),
});

data.get('/', authenticateWebSession(), validateZ('query', listDataSchema), async (c) => {
  const userId = c.get('sub');
  const { since } = c.req.valid('query');

  const [user, ownerIds, owned, incoming] = await Promise.all([
    findUserById(c.env.DB, userId),
    listVisibleOwnerIds(c.env.DB, userId),
    listOwnedPartners(c.env.DB, userId),
    listIncomingPartners(c.env.DB, userId),
  ]);

  if (!user) {
    return c.json({ error: 'Not found' }, 404);
  }

  const batches = await listBatches(c.env.DB, ownerIds, { since });

  return c.json({
    batches: batches
      .map((batch) => {
        const encryptedKey = wrappedKeyForUser(batch.access_keys, userId);
        if (!encryptedKey) {
          return null;
        }

        return {
          user_id: batch.user_id,
          device_id: batch.device_id,
          id: batch.id,
          start_time: batch.start_time,
          end_time: batch.end_time,
          end_hash: endHashHex(batch.end_hash),
          version: batch.version,
          url: batchObject(c.env, batch).url,
          encrypted_key: encryptedKey,
          created_at: batch.created_at,
        };
      })
      .filter((item) => item !== null),
    user: serializeUser(user),
    watching: serializeWatching(incoming),
    watchers: serializeWatchers(owned),
  });
});

export default data;
