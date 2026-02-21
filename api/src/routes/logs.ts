import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { generateDownloadUrl } from '../lib/r2';
import { createLogSchema, listLogsSchema } from '../lib/schemas';

const logs = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /log - Create accountability log entry
 */
logs.post('/', authenticate, async (c) => {
  const parsed = createLogSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: parsed.error.flatten() }, 400);
  
  const userId = c.get('userId');
  const { type, device_id, image_id, metadata } = parsed.data;
  
  const logId = uuidv4();
  const createdAt = new Date().toISOString();
  
  await c.env.DB.prepare(
    `INSERT INTO logs (id, user_id, device_id, image_id, type, metadata, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?)`
  ).bind(logId, userId, device_id, image_id ?? null, type, metadata ? JSON.stringify(metadata) : null, createdAt).run();
  
  return c.json({ id: logId, created_at: createdAt }, 201);
});

/**
 * GET /log - Query accountability logs
 */
logs.get('/', authenticate, async (c) => {
  const parsed = listLogsSchema.safeParse(c.req.query());
  if (!parsed.success) return c.json({ error: parsed.error.flatten() }, 400);
  
  const userId = c.get('userId');
  const { device_id, type, cursor, limit } = parsed.data;
  
  let query = 'SELECT id, type, device_id, image_id, metadata, created_at FROM logs WHERE user_id = ?';
  const params: unknown[] = [userId];
  
  if (device_id) { query += ' AND device_id = ?'; params.push(device_id); }
  if (type) { query += ' AND type = ?'; params.push(type); }
  if (cursor) { query += ' AND created_at < ?'; params.push(cursor); }
  
  query += ' ORDER BY created_at DESC LIMIT ?';
  params.push(limit + 1);
  
  const result = await c.env.DB.prepare(query).bind(...params).all();
  const hasMore = result.results.length > limit;
  const items = hasMore ? result.results.slice(0, limit) : result.results;
  
  const itemsWithUrls = await Promise.all(items.map(async (log) => {
    let imageUrl: string | null = null;
    if (log.image_id) {
      const image = await c.env.DB.prepare('SELECT r2_key FROM images WHERE id = ?')
        .bind(log.image_id).first();
      if (image) imageUrl = await generateDownloadUrl(c.env, image.r2_key as string);
    }
    return {
      id: log.id,
      type: log.type,
      device_id: log.device_id,
      image_url: imageUrl,
      metadata: log.metadata ? JSON.parse(log.metadata as string) : null,
      created_at: log.created_at,
    };
  }));
  
  return c.json({
    items: itemsWithUrls,
    ...(hasMore && { next_cursor: items[items.length - 1].created_at }),
  });
});

export default logs;
