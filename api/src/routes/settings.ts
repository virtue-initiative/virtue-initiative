import z from 'zod';
import { Hono } from 'hono';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { settingsSchema } from '../lib/schemas';
import { getSettings, saveSettings } from '../lib/db';

const DEFAULTS = { name: null, timezone: 'UTC', retention_days: 30 };

const settings = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /settings - Create or replace user settings (JSON blob)
 */
settings.post('/', authenticate, async (c) => {
  const parsed = settingsSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const userId = c.get('userId');
  const updatedAt = new Date().toISOString();

  // Fetch existing data to merge with
  const existing = await getSettings(c.env.DB, userId);

  const current = existing ? JSON.parse(existing.data) : { ...DEFAULTS };
  const merged = { ...current, ...parsed.data };

  await saveSettings(c.env.DB, userId, JSON.stringify(merged), updatedAt);

  return c.json(merged);
});

/**
 * GET /settings - Get user settings
 */
settings.get('/', authenticate, async (c) => {
  const userId = c.get('userId');

  const row = await getSettings(c.env.DB, userId);

  return c.json(row ? JSON.parse(row.data) : { ...DEFAULTS });
});

export default settings;
