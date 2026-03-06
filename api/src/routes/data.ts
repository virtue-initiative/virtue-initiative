import { Hono } from 'hono';
import { z } from 'zod';
import { authenticate } from '../middleware/auth';
import { canViewUserData, listBatches, listDeviceLogs } from '../lib/db';
import { validateZ } from '../middleware/validation';
import { Env, Variables } from '../types/bindings';

const data = new Hono<{ Bindings: Env; Variables: Variables }>();

const listDataSchema = z.object({
  device_id: z.uuid().optional(),
  user: z.uuid().optional(),
  cursor: z.coerce.number().int().nonnegative().optional(),
  limit: z.coerce.number().int().positive().max(100).optional().default(50),
});

data.get('/', authenticate('access'), validateZ('query', listDataSchema), async (c) => {
  const requesterId = c.get('sub');
  const { device_id, user, cursor, limit } = c.req.valid('query');
  const targetUserId = user ?? requesterId;

  if (!(await canViewUserData(c.env.DB, targetUserId, requesterId))) {
    return c.json({ error: 'Forbidden' }, 403);
  }

  const fetchLimit = limit + 1;
  const [batches, logs] = await Promise.all([
    listBatches(c.env.DB, [targetUserId], { deviceId: device_id, cursor }, fetchLimit),
    listDeviceLogs(c.env.DB, [targetUserId], { deviceId: device_id, cursor }, fetchLimit),
  ]);

  const combined = [
    ...batches.map((batch) => ({
      created_at: batch.created_at,
      kind: 'batch' as const,
      value: batch,
    })),
    ...logs.map((log) => ({ created_at: log.created_at, kind: 'log' as const, value: log })),
  ].sort((a, b) => b.created_at - a.created_at);

  const page = combined.slice(0, limit);
  const nextCursor = combined.length > limit ? page[page.length - 1]?.created_at : undefined;

  return c.json({
    batches: page
      .filter((item) => item.kind === 'batch')
      .map((item) => ({
        device_id: item.value.device_id,
        id: item.value.id,
        start: item.value.start,
        end: item.value.end,
        end_hash: item.value.end_hash,
        url: item.value.url,
      })),
    logs: page
      .filter((item) => item.kind === 'log')
      .map((item) => ({
        device_id: item.value.device_id,
        ts: item.value.ts,
        type: item.value.type,
        data: JSON.parse(item.value.data) as Record<string, unknown>,
      })),
    ...(nextCursor !== undefined ? { next_cursor: nextCursor } : {}),
  });
});

export default data;
