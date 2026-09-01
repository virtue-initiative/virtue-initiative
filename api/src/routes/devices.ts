import { Hono } from 'hono';
import { authenticateWebSession } from '../middleware/auth';
import { rateLimitByDevice, rateLimitByIp } from '../middleware/rate-limit';
import { validateZ } from '../middleware/validation';
import {
  approveDeviceAuthCode,
  deleteDeviceById,
  findDeviceAuthCodeByUserCode,
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
import { normalizeUserCode } from '../lib/device-codes';
import { Env, Variables } from '../types/bindings';
import { deviceCodeRequestSchema, updateDeviceSchema } from '../../../shared-web/types';

const devices = new Hono<{ Bindings: Env; Variables: Variables }>();

/** Mounted at `/device-code`, not under `/device`, so the paths match API-043. */
export const deviceCodes = new Hono<{ Bindings: Env; Variables: Variables }>();

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

/**
 * The one message every device-code failure returns (API-046). Distinguishing
 * "no such code" from "expired" from "already approved" would let someone
 * guessing codes learn which guesses were close.
 */
const INVALID_CODE_MESSAGE = 'That code is not valid. It may have expired.';

/**
 * POST /device-code/lookup (API-046) - Resolve a code the user typed into the
 * name and platform the device chose, so they can see what they are about to add
 * before they add it.
 *
 * `rateLimitByDevice()` keys on `c.get('sub')`, which after web auth is the user
 * id; the name is a misnomer here. Both limits matter: a guessed code signs a
 * stranger's device in to the guesser's account.
 */
deviceCodes.post(
  '/lookup',
  authenticateWebSession(),
  rateLimitByDevice(),
  rateLimitByIp(),
  validateZ('json', deviceCodeRequestSchema),
  async (c) => {
    const userCode = normalizeUserCode(c.req.valid('json').user_code);
    if (!userCode) {
      return c.json({ error: INVALID_CODE_MESSAGE }, 404);
    }

    const row = await findDeviceAuthCodeByUserCode(c.env.DB, userCode);
    if (!row || row.approved_by || row.consumed_at !== null || row.expires_at <= Date.now()) {
      return c.json({ error: INVALID_CODE_MESSAGE }, 404);
    }

    return c.json({ name: row.name, platform: row.platform, expires_at: row.expires_at });
  },
);

/**
 * POST /device-code/approve (API-047) - Sign the pending device in to this
 * account. The device itself creates its row on its next poll.
 */
deviceCodes.post(
  '/approve',
  authenticateWebSession(),
  rateLimitByDevice(),
  rateLimitByIp(),
  validateZ('json', deviceCodeRequestSchema),
  async (c) => {
    const userCode = normalizeUserCode(c.req.valid('json').user_code);
    if (!userCode) {
      return c.json({ error: INVALID_CODE_MESSAGE }, 404);
    }

    const row = await findDeviceAuthCodeByUserCode(c.env.DB, userCode);
    if (!row || row.consumed_at !== null || row.expires_at <= Date.now()) {
      return c.json({ error: INVALID_CODE_MESSAGE }, 404);
    }

    if (row.approved_by) {
      return c.json({ error: 'That code was already approved.' }, 409);
    }

    const approved = await approveDeviceAuthCode(c.env.DB, userCode, c.get('sub'), Date.now());
    if (!approved) {
      // Another request approved it between the read above and this update.
      return c.json({ error: 'That code was already approved.' }, 409);
    }

    return c.json({ name: row.name, platform: row.platform });
  },
);

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
