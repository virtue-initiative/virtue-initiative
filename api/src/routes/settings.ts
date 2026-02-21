import { Hono } from 'hono';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { settingsSchema } from '../lib/schemas';

const DEFAULTS = { name: null, timezone: 'UTC', retention_days: 30 };

const settings = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /settings - Create or replace user settings (JSON blob)
 */
settings.post('/', authenticate, async (c) => {
  const parsed = settingsSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: parsed.error.flatten() }, 400);
  
  const userId = c.get('userId');
  const updatedAt = new Date().toISOString();
  
  // Fetch existing data to merge with
  const existing = await c.env.DB.prepare(
    'SELECT data FROM settings WHERE user_id = ?'
  ).bind(userId).first<{ data: string }>();
  
  const current = existing ? JSON.parse(existing.data) : { ...DEFAULTS };
  const merged = { ...current, ...parsed.data };
  
  await c.env.DB.prepare(
    `INSERT INTO settings (user_id, data, updated_at) VALUES (?, ?, ?)
     ON CONFLICT(user_id) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at`
  ).bind(userId, JSON.stringify(merged), updatedAt).run();
  
  return c.json(merged);
});

/**
 * GET /settings - Get user settings
 */
settings.get('/', authenticate, async (c) => {
  const userId = c.get('userId');
  
  const row = await c.env.DB.prepare(
    'SELECT data FROM settings WHERE user_id = ?'
  ).bind(userId).first<{ data: string }>();
  
  return c.json(row ? JSON.parse(row.data) : { ...DEFAULTS });
});

export default settings;
