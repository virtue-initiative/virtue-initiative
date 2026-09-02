import { Context, Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { z } from 'zod';
import { authenticateDeviceSession } from '../middleware/auth';
import { CURRENT_API_VERSION } from '../lib/api-version';
import { rateLimitByDevice, rateLimitByIp } from '../middleware/rate-limit';
import { validateZ } from '../middleware/validation';
import {
  claimDeviceAuthCode,
  createBatch,
  createDevice,
  createDeviceAuthCode,
  createSessionRecord,
  deleteDeviceSessionsByDeviceId,
  findDeviceAuthCodeByDeviceCodeHash,
  findDeviceById,
  findUserById,
  listAcceptedNotificationTargetsForUser,
  listBatchAccessRecipientsForOwner,
  markDeviceDeleted,
} from '../lib/db';
import { sendEmail } from '../lib/email';
import { renderDeviceLoggedOutTemplate } from '../lib/email/templates';
import { hashGet, hashReset } from '../lib/hash-server';
import { encodeBase64 } from '../lib/encoding';
import { jsonField } from '../lib/form-validation';
import { generateToken } from '../lib/jwt';
import { putObject } from '../lib/r2';
import { notifyPartnersAboutRiskLog, riskToSeverity } from '../lib/tamper';
import { generateOpaqueToken, hashOpaqueToken } from '../lib/tokens';
import { describeRequestOrigin, formatUserCode, generateUserCode } from '../lib/device-codes';
import { DEVICE_CODE_TTL_MS } from '../lib/email-domain';
import { Env, Variables } from '../types/bindings';
import { verifyUserCredentials } from '../lib/credentials';

const deviceOnly = new Hono<{ Bindings: Env; Variables: Variables }>();

const HASH_TOKEN_TTL_SECONDS = 60 * 60;
const DEVICE_REFRESH_TOKEN_TTL_SECONDS = 1000 * 365 * 24 * 60 * 60;

const registerDeviceSchema = z.object({
  email: z.email(),
  password_auth: z.base64(),
  name: z.string().min(1),
  platform: z.string().min(1),
});

const startDeviceCodeSchema = registerDeviceSchema.pick({ name: true, platform: true });

const pollDeviceCodeSchema = z.object({
  device_code: z.string().min(1),
});

const notifyEntrySchema = z.object({
  ts: z.number().int().nonnegative(),
  type: z.string().min(1),
  risk: z.number().min(0).max(1),
  title: z.string().optional(),
  details: z.string().optional(),
});

const eventCountsSchema = z.object({
  total: z.number().int().nonnegative(),
  high: z.number().int().nonnegative(),
  medium: z.number().int().nonnegative(),
  screenshot: z.number().int().nonnegative(),
});

const batchMetadataSchema = z.object({
  start_time: z.number().int().nonnegative(),
  end_time: z.number().int().nonnegative(),
  access_keys: z.record(z.uuid(), z.base64()),
  event_counts: eventCountsSchema,
  notifications: z.array(notifyEntrySchema).optional().default([]),
});

const uploadBatchSchema = z.object({
  metadata: jsonField(batchMetadataSchema, 'metadata'),
  file: z
    .instanceof(File)
    .refine((file) => file.size > 0, { message: 'File is empty' })
    .refine((file) => file.size <= 100 * 1024 * 1024, { message: 'File exceeds 100MB limit' }),
});

async function createDeviceSession(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  deviceId: string,
) {
  const refreshToken = generateOpaqueToken('device_session');
  const now = Date.now();

  await createSessionRecord(c.env.DB, {
    session_type: 'device',
    device_id: deviceId,
    refresh_token_hash: hashOpaqueToken(refreshToken),
    expires_at: now + DEVICE_REFRESH_TOKEN_TTL_SECONDS * 1000,
    created_at: now,
  });

  return refreshToken;
}

/**
 * Builds the DeviceSettings embedded in POST /d/device, GET /d/device, and POST /d/batch
 * responses — the one canonical place a device's wrapping keys and a fresh hash-server
 * token (DeviceSettings.hash_token) are assembled.
 */
async function buildDeviceSettings(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  device: { id: string; owner: string; name: string; platform: string },
) {
  const hashBaseUrl = c.env.HASH_SERVER_URL?.trim();
  if (!hashBaseUrl) {
    return null; // caller returns the existing 500
  }

  const recipients = await listBatchAccessRecipientsForOwner(c.env.DB, device.owner);
  const hashToken = await generateToken(
    'device',
    device.id,
    c.env.JWT_PRIVATE_KEY,
    HASH_TOKEN_TTL_SECONDS,
  );

  return {
    id: device.id,
    name: device.name,
    platform: device.platform,
    wrapping_keys: recipients.map((recipient) => ({
      user_id: recipient.id,
      pub_key: encodeBase64(recipient.pub_key!),
    })),
    hash_base_url: hashBaseUrl,
    hash_token: hashToken,
  };
}

/**
 * POST /d/device - Register a device using the owner's email + password_auth
 * (the same credential material POST /login accepts). No web session required —
 * this is the device's first call, made before it has any session of its own.
 */
deviceOnly.post('/device', validateZ('json', registerDeviceSchema), async (c) => {
  const { email, password_auth, name, platform } = c.req.valid('json');
  const result = await verifyUserCredentials(c.env.DB, email, password_auth);

  if (result.status === 'invalid') {
    return c.json({ error: 'Invalid email or password' }, 401);
  }

  if (result.status === 'unverified') {
    return c.json({ error: 'Please verify your email before logging in.' }, 403);
  }

  const owner = result.user.id;
  const id = uuidv4();

  await createDevice(c.env.DB, { id, owner, name, platform });
  const refreshToken = await createDeviceSession(c, id);

  const settings = await buildDeviceSettings(c, { id, owner, name, platform });
  if (!settings) {
    return c.json({ error: 'Hash server not configured' }, 500);
  }

  return c.json({ token: refreshToken, settings }, 200);
});

const DEVICE_CODE_POLL_INTERVAL_SECONDS = 5;
const USER_CODE_COLLISION_RETRIES = 5;

/**
 * POST /d/device-code (API-044) - Start a passwordless pairing. Unauthenticated,
 * like POST /d/device: this is the device's first call. It returns a short code
 * for the user to read out to their web session, plus the `dpc_` secret the
 * device polls with.
 */
deviceOnly.post(
  '/device-code',
  rateLimitByIp(),
  validateZ('json', startDeviceCodeSchema),
  async (c) => {
    const { name, platform } = c.req.valid('json');
    const now = Date.now();
    const expiresAt = now + DEVICE_CODE_TTL_MS;
    const deviceCode = generateOpaqueToken('device_pairing');
    const requestedFrom = describeRequestOrigin(c.req.raw);

    for (let attempt = 0; ; attempt += 1) {
      const userCode = generateUserCode();

      try {
        await createDeviceAuthCode(c.env.DB, {
          id: uuidv4(),
          user_code: userCode,
          device_code_hash: hashOpaqueToken(deviceCode),
          name,
          platform,
          expires_at: expiresAt,
          created_at: now,
          requested_from: requestedFrom,
        });

        return c.json(
          {
            user_code: formatUserCode(userCode),
            device_code: deviceCode,
            expires_at: expiresAt,
            interval: DEVICE_CODE_POLL_INTERVAL_SECONDS,
          },
          200,
        );
      } catch (error) {
        // A live code collided. Redraw a few times before giving up — with 30^6
        // codes and a 10-minute TTL this should effectively never happen twice.
        if (attempt >= USER_CODE_COLLISION_RETRIES) {
          throw error;
        }
      }
    }
  },
);

/**
 * POST /d/device-code/poll (API-045) - The device asks whether its pairing has
 * been approved yet. The approved branch is where the device row is actually
 * created, so an approval nobody collects leaves nothing behind.
 */
deviceOnly.post(
  '/device-code/poll',
  rateLimitByIp(),
  validateZ('json', pollDeviceCodeSchema),
  async (c) => {
    const { device_code } = c.req.valid('json');
    const hash = hashOpaqueToken(device_code);
    const now = Date.now();

    const row = await findDeviceAuthCodeByDeviceCodeHash(c.env.DB, hash);
    if (!row || row.consumed_at !== null || row.expires_at <= now) {
      return c.json({ error: 'expired' }, 410);
    }

    if (!row.approved_by) {
      return c.json({ status: 'pending' }, 202);
    }

    // Guard the race between two concurrent polls: only the UPDATE that flips
    // consumed_at from NULL proceeds to create the device.
    const claimed = await claimDeviceAuthCode(c.env.DB, hash, now);
    if (!claimed) {
      return c.json({ error: 'expired' }, 410);
    }

    const owner = await findUserById(c.env.DB, row.approved_by);
    if (!owner) {
      return c.json({ error: 'expired' }, 410);
    }

    const id = uuidv4();
    await createDevice(c.env.DB, {
      id,
      owner: owner.id,
      name: row.name,
      platform: row.platform,
    });
    const refreshToken = await createDeviceSession(c, id);

    const settings = await buildDeviceSettings(c, {
      id,
      owner: owner.id,
      name: row.name,
      platform: row.platform,
    });
    if (!settings) {
      return c.json({ error: 'Hash server not configured' }, 500);
    }

    return c.json({ token: refreshToken, settings, account_email: owner.email }, 200);
  },
);

/**
 * GET /d/device - Refresh settings and mint a fresh hash-server token for the
 * authenticated device. Used by the client as the manual/periodic refresh path
 * when its cached hash token goes stale without a batch upload in between.
 */
deviceOnly.get('/device', authenticateDeviceSession(), rateLimitByDevice(), async (c) => {
  const device = await findDeviceById(c.env.DB, c.get('sub'));

  if (!device) {
    return c.json({ error: 'Not found' }, 404);
  }

  const settings = await buildDeviceSettings(c, device);
  if (!settings) {
    return c.json({ error: 'Hash server not configured' }, 500);
  }

  return c.json(settings);
});

/**
 * POST /d/logout - Revoke the authenticated device's session and soft-delete it.
 *
 * Clears the device's hash-chain state as a best-effort cleanup; batches/screenshots
 * and the device row itself are untouched (that's the manual hard-delete flow).
 *
 * A logout takes the device out of monitoring, so the owner and their accepted
 * watchers are emailed about it (API-037). Both the hash reset and the emails are
 * best-effort: the logout itself is already committed.
 */
deviceOnly.post('/logout', authenticateDeviceSession(), async (c) => {
  const deviceId = c.get('sub');
  const now = Date.now();

  const device = await findDeviceById(c.env.DB, deviceId);

  await deleteDeviceSessionsByDeviceId(c.env.DB, deviceId);
  await markDeviceDeleted(c.env.DB, deviceId, now);

  try {
    await hashReset(c.env, deviceId);
  } catch {
    // Non-fatal — logout/soft-delete already committed; hash state is best-effort.
  }

  if (device) {
    try {
      await notifyAboutDeviceLogout(c, device);
    } catch {
      // Best-effort — the logout itself already committed successfully.
    }
  }

  return c.body(null, 204);
});

async function notifyAboutDeviceLogout(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  device: { id: string; owner: string; name: string; platform: string },
) {
  const owner = await findUserById(c.env.DB, device.owner);

  if (owner) {
    const email = renderDeviceLoggedOutTemplate({
      appName: c.env.APP_NAME,
      appUrl: c.env.APP_URL,
      recipientName: owner.name,
      deviceName: device.name,
      devicePlatform: device.platform,
    });
    await sendEmail({
      env: c.env,
      db: c.env.DB,
      kind: 'device_logout',
      recipient: owner.email,
      allowUnverified: true,
      subject: email.subject,
      text: email.text,
      html: email.html,
      related_user_id: owner.id,
      metadata: { deviceId: device.id, deviceName: device.name },
    });
  }

  const targets = await listAcceptedNotificationTargetsForUser(c.env.DB, device.owner);
  for (const target of targets) {
    if (target.settings.email_frequency === 'none') {
      continue;
    }

    const email = renderDeviceLoggedOutTemplate({
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
      kind: 'device_logout',
      recipient: target.watcher_email,
      allowUnverified: true,
      subject: email.subject,
      text: email.text,
      html: email.html,
      related_user_id: device.owner,
      related_partnership_id: target.partnership_id,
      metadata: { deviceId: device.id, deviceName: device.name, forPartner: true },
    });
  }
}

/**
 * POST /d/batch - Upload an encrypted batch blob for the authenticated device.
 *
 * `metadata.notifications` (if any) trigger the same best-effort partner alert
 * email that POST /d/notify used to send standalone, after the batch is durably
 * persisted.
 */
deviceOnly.post(
  '/batch',
  authenticateDeviceSession(),
  rateLimitByDevice(),
  validateZ('form', uploadBatchSchema),
  async (c) => {
    const device = await findDeviceById(c.env.DB, c.get('sub'));

    if (!device) {
      return c.json({ error: 'Not found' }, 404);
    }

    const { metadata, file } = c.req.valid('form');
    const { start_time, end_time, access_keys, event_counts, notifications } = metadata;

    const hashState = await hashGet(c.env, device.id);
    const endHash = hashState.hash;
    const batchId = uuidv4();
    const key = `user/${device.owner}/batches/${batchId}.enc`;
    const url = `${c.env.R2_URL}/${key}`;
    const createdAt = Date.now();

    await putObject(c.env, key, await file.arrayBuffer(), 'application/octet-stream');
    await createBatch(c.env.DB, {
      id: batchId,
      user_id: device.owner,
      device_id: device.id,
      url,
      start_time,
      end_time,
      end_hash: endHash,
      access_keys: JSON.stringify(access_keys),
      version: CURRENT_API_VERSION,
      high_risk_count: event_counts.high,
      medium_risk_count: event_counts.medium,
      created_at: createdAt,
    });
    await hashReset(c.env, device.id);

    for (const notification of notifications) {
      try {
        const providedTitle = notification.title?.trim();
        const providedDetails = notification.details?.trim();
        await notifyPartnersAboutRiskLog(c.env.DB, c.env, {
          logId: uuidv4(),
          appUrl: c.env.APP_URL,
          userId: device.owner,
          deviceName: device.name,
          severity: riskToSeverity(notification.risk) ?? 'info',
          risk: notification.risk,
          title:
            providedTitle && providedTitle.length > 0
              ? providedTitle
              : `Device reported ${notification.type.replaceAll('_', ' ')}.`,
          details: providedDetails && providedDetails.length > 0 ? providedDetails : null,
          happenedAt: notification.ts,
        });
      } catch {
        // Best-effort — the batch itself already committed successfully.
      }
    }

    const settings = await buildDeviceSettings(c, device);
    if (!settings) {
      return c.json({ error: 'Hash server not configured' }, 500);
    }

    return c.json({ id: batchId, start_time, end_time, end_hash: endHash, url, settings }, 200);
  },
);

export default deviceOnly;
