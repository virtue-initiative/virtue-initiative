import z from 'zod';
import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { generateDownloadUrl } from '../lib/r2';
import { createLogSchema, listLogsSchema } from '../lib/schemas';
import { createLog, queryLogs, findImageById } from '../lib/db';

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

  const userId = c.get('userId');
  const { device_id, type, cursor, limit } = parsed.data;

  const { items, hasMore } = await queryLogs(c.env.DB, userId, { device_id, type, cursor }, limit);

  const itemsWithUrls = await Promise.all(
    items.map(async (log) => {
      let imageUrl: string | null = null;
      if (log.image_id) {
        const image = await findImageById(c.env.DB, log.image_id);
        if (image) imageUrl = await generateDownloadUrl(c.env, image.r2_key);
      }
      return {
        id: log.id,
        type: log.type,
        device_id: log.device_id,
        image_url: imageUrl,
        metadata: log.metadata ? JSON.parse(log.metadata) : null,
        created_at: log.created_at,
      };
    }),
  );

  return c.json({
    items: itemsWithUrls,
    ...(hasMore && { next_cursor: items[items.length - 1].created_at }),
  });
});

export default logs;
