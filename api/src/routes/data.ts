import { Hono } from 'hono';
import { z } from 'zod';
import { authenticateWebSession } from '../middleware/auth';
import { canViewUserData, listBatches } from '../lib/db';
import { validateZ } from '../middleware/validation';
import { Env, Variables } from '../types/bindings';

const data = new Hono<{ Bindings: Env; Variables: Variables }>();

function getEncryptedKeyForUser(rawAccessKeys: string, userId: string) {
  try {
    const payload = JSON.parse(rawAccessKeys) as {
      keys?: Array<{ user_id?: string; hpke_key?: string }>;
    };
    return (
      payload.keys?.find((key) => key.user_id === userId && typeof key.hpke_key === 'string')
        ?.hpke_key ?? null
    );
  } catch {
    return null;
  }
}

const listDataSchema = z.object({
  device_id: z.uuid().optional(),
  user: z.uuid().optional(),
  since: z.coerce.number().int().nonnegative().optional().default(0),
});

data.get('/', authenticateWebSession(), validateZ('query', listDataSchema), async (c) => {
  const requesterId = c.get('sub');
  const { device_id, user, since } = c.req.valid('query');
  const targetUserId = user ?? requesterId;

  if (!(await canViewUserData(c.env.DB, targetUserId, requesterId))) {
    return c.json({ error: 'Forbidden' }, 403);
  }

  const batches = await listBatches(c.env.DB, [targetUserId], { deviceId: device_id, since });

  return c.json({
    batches: batches
      .map((batch) => {
        const encryptedKey = getEncryptedKeyForUser(batch.access_keys, requesterId);
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
  });
});

export default data;
