import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import z from 'zod';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { postAlertLogSchema, listAlertLogsSchema } from '../lib/schemas';
import { findDevice, createAlertLog, listAlertLogs, findAcceptedPartnership } from '../lib/db';

const logs = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /logs — Submit a single unencrypted alert log entry from a device.
 */
logs.post('/', authenticate, async (c) => {
  let body: unknown;
  try {
    body = await c.req.json();
  } catch {
    return c.json({ error: 'Expected JSON body' }, 400);
  }

  const parsed = postAlertLogSchema.safeParse(body);
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const userId = c.get('userId');
  const { device_id, created_at, kind, metadata } = parsed.data;

  const device = await findDevice(c.env.DB, device_id, userId);
  if (!device) return c.json({ error: 'Device not found' }, 404);

  const id = uuidv4();

  await createAlertLog(c.env.DB, id, userId, device_id, kind, JSON.stringify(metadata), created_at);

  return c.json(
    {
      log: {
        id,
        device_id,
        kind,
        metadata,
        created_at,
      },
    },
    201,
  );
});

/**
 * GET /logs — List alert log entries for the authenticated user (or a partner with view_data).
 */
logs.get('/', authenticate, async (c) => {
  const parsed = listAlertLogsSchema.safeParse(c.req.query());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const requesterId = c.get('userId');
  const { device_id, user, cursor, limit } = parsed.data;
  const targetId = user ?? requesterId;

  if (targetId !== requesterId) {
    const partnership = await findAcceptedPartnership(c.env.DB, targetId, requesterId);
    if (!partnership) return c.json({ error: 'Forbidden' }, 403);
    const perms = JSON.parse(partnership.permissions);
    if (!perms.view_data) return c.json({ error: 'Forbidden' }, 403);
  }

  const { items, hasMore } = await listAlertLogs(c.env.DB, targetId, { device_id, cursor }, limit);

  return c.json({
    items: items.map((item) => ({
      ...item,
      metadata: JSON.parse(item.metadata) as [string, string][],
    })),
    ...(hasMore && { next_cursor: items[items.length - 1].created_at }),
  });
});

export default logs;
