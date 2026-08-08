import { Hono } from 'hono';
import { authenticateWebSession } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import {
  deleteDeviceById,
  findOwnedDevice,
  findUserById,
  listBatchUrlsForDevice,
  listAcceptedNotificationTargetsForUser,
  updateDevice,
} from '../lib/db';
import { sendEmail } from '../lib/email';
import { renderDeviceDeletedTemplate } from '../lib/email/templates';
import { deleteObject } from '../lib/r2';
import { buildDeviceViews } from '../lib/views';
import { Env, Variables } from '../types/bindings';
import { updateDeviceSchema, type PatchDeviceResponse } from '../../../shared-web/types';

const devices = new Hono<{ Bindings: Env; Variables: Variables }>();
function getAppUrl(env: Env) {
  return env.APP_URL;
}

devices.get('/', authenticateWebSession(), async (c) => {
  const views = await buildDeviceViews(c.env, c.get('sub'));
  return c.json(views);
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
