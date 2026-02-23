import z from 'zod';
import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { createLogSchema, listLogsSchema } from '../lib/schemas';
import { createLog, queryLogs, findAcceptedPartnership } from '../lib/db';

const logs = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /log - Create accountability log entry
 */
logs.post('/', authenticate, async (c) => {
  const parsed = createLogSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const userId = c.get('userId');
  const { type, device_id, image_id, metadata } = parsed.data;

  const logId = uuidv4();
  const createdAt = new Date().toISOString();

  await createLog(
    c.env.DB,
    logId,
    userId,
    device_id,
    image_id ?? null,
    type,
    metadata ? JSON.stringify(metadata) : null,
    createdAt,
  );

  return c.json({ id: logId, created_at: createdAt }, 201);
});

/**
 * GET /log - Query accountability logs
 */
logs.get('/', authenticate, async (c) => {
  const parsed = listLogsSchema.safeParse(c.req.query());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const requesterId = c.get('userId');
  const { device_id, type, user, cursor, limit } = parsed.data;
  const targetId = user ?? requesterId;

  if (targetId !== requesterId) {
    const partnership = await findAcceptedPartnership(c.env.DB, targetId, requesterId);
    if (!partnership) return c.json({ error: 'Forbidden' }, 403);
    const perms = JSON.parse(partnership.permissions);
    if (!perms.view_logs) return c.json({ error: 'Forbidden' }, 403);
  }

  const { items, hasMore } = await queryLogs(c.env.DB, targetId, { device_id, type, cursor }, limit);

  const itemsWithUrls = items.map((log) => {
    const imageUrl = log.image_id
      ? `${new URL(c.req.url).origin}/image/${log.image_id}`
      : null;
    return {
      id: log.id,
      type: log.type,
      device_id: log.device_id,
      image_url: imageUrl,
      metadata: log.metadata ? JSON.parse(log.metadata) : null,
      created_at: log.created_at,
    };
  });

  return c.json({
    items: itemsWithUrls,
    ...(hasMore && { next_cursor: items[items.length - 1].created_at }),
  });
});

export default logs;
