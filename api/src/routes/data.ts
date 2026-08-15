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
import { serializeUser, serializeWatchers, serializeWatching } from '../lib/serializers';
import { validateZ } from '../middleware/validation';
import { Env, Variables } from '../types/bindings';

const data = new Hono<{ Bindings: Env; Variables: Variables }>();

function getEncryptedKeyForUser(rawAccessKeys: string, userId: string) {
  try {
    const accessKeys = JSON.parse(rawAccessKeys) as Record<string, string>;
    return accessKeys[userId] ?? null;
  } catch {
    return null;
  }
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
        const encryptedKey = getEncryptedKeyForUser(batch.access_keys, userId);
        if (!encryptedKey) {
          return null;
        }

        return {
          device_id: batch.device_id,
          id: batch.id,
          start_time: batch.start_time,
          end_time: batch.end_time,
          end_hash: batch.end_hash,
          url: batch.url,
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
