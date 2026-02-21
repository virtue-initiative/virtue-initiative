import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { createDeviceSchema, updateDeviceSchema } from '../lib/schemas';

const devices = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /device - Register new device
 */
devices.post('/', authenticate, async (c) => {
  const parsed = createDeviceSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: parsed.error.flatten() }, 400);
  
  const { name, platform, avg_interval_seconds } = parsed.data;
  const userId = c.get('userId');
  const deviceId = uuidv4();
  const createdAt = new Date().toISOString();
  
  await c.env.DB.prepare(
    `INSERT INTO devices (id, user_id, name, platform, avg_interval_seconds, enabled, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?)`
  ).bind(deviceId, userId, name, platform, avg_interval_seconds, 1, createdAt).run();
  
  return c.json({ id: deviceId, created_at: createdAt }, 201);
});

/**
 * GET /device - List user's devices
 */
devices.get('/', authenticate, async (c) => {
  const userId = c.get('userId');
  
  const result = await c.env.DB.prepare(
    `SELECT id, name, platform, last_seen_at, last_upload_at, avg_interval_seconds, enabled
     FROM devices WHERE user_id = ? ORDER BY created_at DESC`
  ).bind(userId).all();
  
  return c.json(result.results.map((device) => {
    let status = 'offline';
    if (device.last_seen_at) {
      const diffMinutes = (Date.now() - new Date(device.last_seen_at as string).getTime()) / 60000;
      if (diffMinutes < (device.avg_interval_seconds as number) / 60 * 2) status = 'online';
    }
    return {
      id: device.id,
      name: device.name,
      platform: device.platform,
      last_seen_at: device.last_seen_at,
      last_upload_at: device.last_upload_at,
      interval_seconds: device.avg_interval_seconds,
      status,
      enabled: device.enabled === 1,
    };
  }));
});

/**
 * PATCH /device/:id - Update device configuration
 */
devices.patch('/:id', authenticate, async (c) => {
  const parsed = updateDeviceSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: parsed.error.flatten() }, 400);
  
  const userId = c.get('userId');
  const deviceId = c.req.param('id');
  
  const device = await c.env.DB.prepare(
    'SELECT id FROM devices WHERE id = ? AND user_id = ?'
  ).bind(deviceId, userId).first();
  if (!device) return c.json({ error: 'Device not found' }, 404);
  
  const { name, interval_seconds, enabled } = parsed.data;
  const updates: string[] = [];
  const params: unknown[] = [];
  
  if (name !== undefined) { updates.push('name = ?'); params.push(name); }
  if (interval_seconds !== undefined) { updates.push('avg_interval_seconds = ?'); params.push(interval_seconds); }
  if (enabled !== undefined) { updates.push('enabled = ?'); params.push(enabled ? 1 : 0); }
  
  params.push(deviceId);
  await c.env.DB.prepare(`UPDATE devices SET ${updates.join(', ')} WHERE id = ?`).bind(...params).run();
  
  return c.json({ id: deviceId, updated: true });
});

export default devices;
