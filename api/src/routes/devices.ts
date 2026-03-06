import { Hono } from 'hono';
import { z } from 'zod';
import { authenticate } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import {
  findOwnedDevice,
  listDevicesForOwners,
  listVisibleOwnerIds,
  updateDevice,
} from '../lib/db';
import { Env, Variables } from '../types/bindings';

const devices = new Hono<{ Bindings: Env; Variables: Variables }>();
const ONLINE_WINDOW_MS = 2 * 60 * 60 * 1000;

const updateDeviceSchema = z
  .object({
    name: z.string().min(1).optional(),
    enabled: z.boolean().optional(),
  })
  .refine((data) => Object.keys(data).length > 0, { message: 'No fields to update' });

devices.get('/', authenticate('access'), async (c) => {
  const ownerIds = await listVisibleOwnerIds(c.env.DB, c.get('sub'));
  const rows = await listDevicesForOwners(c.env.DB, ownerIds);

  return c.json(
    rows.map((device) => ({
      id: device.id,
      owner: device.owner,
      name: device.name,
      platform: device.platform,
      last_upload_at: device.last_upload_at,
      status:
        device.last_upload_at && Date.now() - device.last_upload_at < ONLINE_WINDOW_MS
          ? 'online'
          : 'offline',
      enabled: device.enabled === 1,
    })),
  );
});

devices.patch('/:id', authenticate('access'), validateZ('json', updateDeviceSchema), async (c) => {
  const deviceId = c.req.param('id');
  const device = await findOwnedDevice(c.env.DB, deviceId, c.get('sub'));

  if (!device) {
    return c.json({ error: 'Not found' }, 404);
  }

  const { name, enabled } = c.req.valid('json');
  await updateDevice(c.env.DB, deviceId, { name, enabled });

  return c.json({ id: deviceId, updated: true });
});

export default devices;
