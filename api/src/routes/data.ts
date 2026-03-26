import { Hono } from 'hono';
import { z } from 'zod';
import { authenticate } from '../middleware/auth';
import { canViewUserData, listBatches, listDeviceLogs } from '../lib/db';
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
  limit: z.coerce.number().int().positive().max(500).optional().default(250),
});

data.get('/', authenticate('access'), validateZ('query', listDataSchema), async (c) => {
  const requesterId = c.get('sub');
  const { device_id, user, since, limit } = c.req.valid('query');
  const targetUserId = user ?? requesterId;

  if (!(await canViewUserData(c.env.DB, targetUserId, requesterId))) {
    return c.json({ error: 'Forbidden' }, 403);
  }

  const fetchLimit = limit + 1;
  const [batches, logs] = await Promise.all([
    listBatches(c.env.DB, [targetUserId], { deviceId: device_id, since }, fetchLimit),
    listDeviceLogs(c.env.DB, [targetUserId], { deviceId: device_id, since }, fetchLimit),
  ]);

  const combined = [
    ...batches.map((batch) => ({
      created_at: batch.created_at,
      kind: 'batch' as const,
      value: batch,
    })),
    ...logs.map((log) => ({ created_at: log.created_at, kind: 'log' as const, value: log })),
  ].sort((a, b) => a.created_at - b.created_at);

  const page = combined.slice(0, limit);
  const nextSince = combined.length > limit ? page[page.length - 1]?.created_at : undefined;

  return c.json({
    batches: page
      .filter((item) => item.kind === 'batch')
      .map((item) => {
        const encryptedKey = getEncryptedKeyForUser(item.value.access_keys, requesterId);
        if (!encryptedKey) {
          return null;
        }

        return {
          device_id: item.value.device_id,
          id: item.value.id,
          start_time: item.value.start_time,
          end_time: item.value.end_time,
          end_hash: item.value.end_hash,
          url: item.value.url,
          encrypted_key: encryptedKey,
          created_at: item.value.created_at,
        };
      })
      .filter((item) => item !== null),
    logs: page
      .filter((item) => item.kind === 'log')
      .map((item) => ({
        id: item.value.id,
        device_id: item.value.device_id,
        ts: item.value.ts,
        type: item.value.type,
        data: JSON.parse(item.value.data) as Record<string, unknown>,
        created_at: item.value.created_at,
        ...(item.value.risk !== null ? { risk: item.value.risk } : {}),
      })),
    ...(nextSince !== undefined ? { next_since: nextSince } : {}),
  });
});

export default data;
