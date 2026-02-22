import z from 'zod';
import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { createDeviceSchema, updateDeviceSchema } from '../lib/schemas';
import { createDevice, listDevices, findDevice, updateDevice } from '../lib/db';

const devices = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /device - Register new device
 */
devices.post('/', authenticate, async (c) => {
  const parsed = createDeviceSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const { name, platform, avg_interval_seconds } = parsed.data;
  const userId = c.get('userId');
  const deviceId = uuidv4();
  const createdAt = new Date().toISOString();

  await createDevice(c.env.DB, deviceId, userId, name, platform, avg_interval_seconds, createdAt);

  return c.json({ id: deviceId, created_at: createdAt }, 201);
});

/**
 * GET /device - List user's devices
 */
devices.get('/', authenticate, async (c) => {
  const userId = c.get('userId');

  const result = await listDevices(c.env.DB, userId);

  return c.json(
    result.results.map((device) => {
      let status = 'offline';
      if (device.last_seen_at) {
        const diffMinutes = (Date.now() - new Date(device.last_seen_at).getTime()) / 60000;
        if (diffMinutes < (device.avg_interval_seconds / 60) * 2) status = 'online';
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
    }),
  );
});

/**
 * PATCH /device/:id - Update device configuration
 */
devices.patch('/:id', authenticate, async (c) => {
  const parsed = updateDeviceSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const userId = c.get('userId');
  const deviceId = c.req.param('id');

  const device = await findDevice(c.env.DB, deviceId, userId);
  if (!device) return c.json({ error: 'Device not found' }, 404);

  const { name, interval_seconds, enabled } = parsed.data;
  await updateDevice(c.env.DB, deviceId, { name, interval_seconds, enabled });

  return c.json({ id: deviceId, updated: true });
});

export default devices;
