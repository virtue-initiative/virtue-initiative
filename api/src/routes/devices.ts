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
import { hashGetMany } from '../lib/hash-server';
import { sendEmail } from '../lib/email';
import { renderDeviceDeletedTemplate } from '../lib/email/templates';
import { deleteObject } from '../lib/r2';
import { Env, Variables } from '../types/bindings';
import { updateDeviceSchema } from '../../../shared-web/types';

const devices = new Hono<{ Bindings: Env; Variables: Variables }>();
const ONLINE_WINDOW_MS = 2 * 60 * 60 * 1000;

devices.get('/', authenticateWebSession(), async (c) => {
  const ownerIds = await listVisibleOwnerIds(c.env.DB, c.get('sub'));
  const rows = await listDevicesForOwners(c.env.DB, ownerIds);

  let hashInfo = new Map<string, { hash: string; seq: number; last_received: number }>();
  try {
    hashInfo = await hashGetMany(
      c.env,
      rows.map((device) => device.id),
    );
  } catch {
    // Hash server unreachable — fall back to unknown (zero) state for every device
    // below rather than failing the whole listing.
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
        // hash-server's last_received is unix *seconds* (hash-server/SPEC.md's
        // unix_time is a u32, so it can only be seconds); last_hash_at is a
        // DateTime (millisecond Unix timestamp) per SPEC.md, like every other
        // timestamp this API returns.
        last_hash_at: hi ? hi.last_received * 1000 : null,
        pending_count: hi ? hi.seq : 0,
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

    return c.body(null, 204);
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
      appUrl: c.env.APP_URL,
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
      appUrl: c.env.APP_URL,
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
