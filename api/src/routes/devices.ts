import { Hono } from 'hono';
import { authenticateWebSession } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import {
  deleteDeviceById,
  findOwnedDevice,
  findUserById,
  listBatchUrlsForDevice,
  listAcceptedNotificationTargetsForUser,
  listDevicesForOwners,
  listVisibleOwnerIds,
  updateDevice,
} from '../lib/db';
import { localHashInfo } from '../lib/hash-server';
import { sendEmail } from '../lib/email';
import { renderDeviceDeletedTemplate } from '../lib/email/templates';
import { deleteObject } from '../lib/r2';
import { generateToken } from '../lib/jwt';
import { Env, Variables } from '../types/bindings';
import { updateDeviceSchema, type PatchDeviceResponse } from '../../../shared-web/types';
import { getAppUrl } from '../lib/app-url';

const devices = new Hono<{ Bindings: Env; Variables: Variables }>();
const ONLINE_WINDOW_MS = 2 * 60 * 60 * 1000;

devices.get('/', authenticateWebSession(), async (c) => {
  const ownerIds = await listVisibleOwnerIds(c.env.DB, c.get('sub'));
  const rows = await listDevicesForOwners(c.env.DB, ownerIds);

  const hashServerUrl = c.env.HASH_SERVER_URL?.trim() || null;
  const hashInfo = new Map<string, { count: number; hashed_at: number | null }>();

  if (hashServerUrl?.endsWith('/api')) {
    // Hack: when the hash server is this API itself, skip the HTTP round-trip
    // and read the hash state directly from D1.
    await Promise.all(
      rows.map(async (device) => {
        const info = await localHashInfo(c.env.DB, device.id);
        if (info) {
          hashInfo.set(device.id, { count: info.count, hashed_at: info.hashed_at });
        }
      }),
    );
  } else if (hashServerUrl) {
    await Promise.all(
      rows.map(async (device) => {
        try {
          const token = await generateToken('server', device.id, c.env.JWT_PRIVATE_KEY, 60);
          const resp = await fetch(`${hashServerUrl}/hash/info`, {
            headers: { Authorization: `Bearer ${token}` },
          });
          if (resp.ok) {
            const info = (await resp.json()) as { count: number; hashed_at: number | null };
            hashInfo.set(device.id, info);
          }
        } catch {
          // fall back to D1 values for this device
        }
      }),
    );
  }

  return c.json(
    rows.map((device) => {
      const hi = hashInfo.get(device.id);
      return {
        id: device.id,
        owner: device.owner,
        name: device.name,
        platform: device.platform,
        last_upload_at: device.last_upload_at,
        last_hash_at: hi ? hi.hashed_at : device.last_hash_at,
        pending_count: hi ? hi.count : device.pending_count,
        status: device.deleted_at
          ? 'logged_out'
          : device.last_upload_at && Date.now() - device.last_upload_at < ONLINE_WINDOW_MS
            ? 'online'
            : 'offline',
      };
    }),
  );
});

devices.patch(
  '/:id',
  authenticateWebSession(),
  validateZ('json', updateDeviceSchema),
  async (c) => {
    const deviceId = c.req.param('id');
    const device = await findOwnedDevice(c.env.DB, deviceId, c.get('sub'));

    if (!device) {
      return c.json({ error: 'Not found' }, 404);
    }

    const { name } = c.req.valid('json');
    await updateDevice(c.env.DB, deviceId, { name });

    return c.json<PatchDeviceResponse>({ id: deviceId, updated: true });
  },
);

devices.delete('/:id', authenticateWebSession(), async (c) => {
  const deviceId = c.req.param('id');
  const device = await findOwnedDevice(c.env.DB, deviceId, c.get('sub'));

  if (!device) {
    return c.json({ error: 'Not found' }, 404);
  }

  const owner = await findUserById(c.env.DB, c.get('sub'));
  const batchUrls = await listBatchUrlsForDevice(c.env.DB, deviceId);
  await deleteDeviceById(c.env.DB, deviceId);

  const r2Prefix = `${c.env.R2_URL}/`;
  await Promise.all(
    batchUrls
      .map((batch) => batch.url)
      .filter((url) => url.startsWith(r2Prefix))
      .map((url) => deleteObject(c.env, url.slice(r2Prefix.length))),
  );

  if (owner) {
    const email = renderDeviceDeletedTemplate({
      appName: c.env.APP_NAME,
      appUrl: getAppUrl(c.env),
      recipientName: owner.name,
      deviceName: device.name,
      devicePlatform: device.platform,
    });
    await sendEmail({
      env: c.env,
      db: c.env.DB,
      kind: 'device_deleted',
      recipient: owner.email,
      allowUnverified: true,
      subject: email.subject,
      text: email.text,
      html: email.html,
      related_user_id: owner.id,
      metadata: { deviceId: device.id, deviceName: device.name },
    });
  }

  const targets = await listAcceptedNotificationTargetsForUser(c.env.DB, c.get('sub'));
  for (const target of targets) {
    if (target.settings.email_frequency === 'none') {
      continue;
    }

    const email = renderDeviceDeletedTemplate({
      appName: c.env.APP_NAME,
      appUrl: getAppUrl(c.env),
      recipientName: target.watcher_name,
      deviceName: device.name,
      devicePlatform: device.platform,
      ownerName: owner?.name,
      ownerEmail: owner?.email,
      forPartner: true,
    });
    await sendEmail({
      env: c.env,
      db: c.env.DB,
      kind: 'device_deleted',
      recipient: target.watcher_email,
      allowUnverified: true,
      subject: email.subject,
      text: email.text,
      html: email.html,
      related_user_id: c.get('sub'),
      related_partnership_id: target.partnership_id,
      metadata: { deviceId: device.id, deviceName: device.name, forPartner: true },
    });
  }

  return c.body(null, 204);
});

export default devices;
