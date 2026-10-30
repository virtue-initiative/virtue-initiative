import { Context, Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { z } from 'zod';
import { authenticateDeviceSession } from '../middleware/auth';
import { CURRENT_API_VERSION } from '../lib/api-version';
import { rateLimitByDevice } from '../middleware/rate-limit';
import { validateZ } from '../middleware/validation';
import {
  createBatch,
  createDevice,
  createSessionRecord,
  deleteDeviceSessionsByDeviceId,
  findDeviceById,
  findUserById,
  listAcceptedNotificationTargetsForUser,
  listBatchAccessRecipientsForOwner,
  markDeviceDeleted,
} from '../lib/db';
import { encodeAccessKeys } from '../lib/access-keys';
import { batchObject } from '../lib/batch-url';
import { sendEmail } from '../lib/email';
import { renderDeviceLoggedOutTemplate } from '../lib/email/templates';
import { hashGet, hashReset } from '../lib/hash-server';
import { decodeHex, encodeBase64 } from '../lib/encoding';
import { enabledFeatureFlags } from '../lib/feature-flags';
import { jsonField } from '../lib/form-validation';
import { generateToken } from '../lib/jwt';
import { deleteObject, putObject } from '../lib/r2';
import { notifyPartnersAboutRiskLog, riskToSeverity } from '../lib/tamper';
import { generateOpaqueToken, hashOpaqueToken } from '../lib/tokens';
import { Env, Variables } from '../types/bindings';
import { verifyUserCredentials } from '../lib/credentials';
import { fillNewsletterLocation, requestLocation } from '../lib/newsletter';
import { unverifiedLoginResponse } from '../lib/email-verification';

const deviceOnly = new Hono<{ Bindings: Env; Variables: Variables }>();

// API-002: outlives the hourly batch upload, which mints the next one.
const HASH_TOKEN_TTL_SECONDS = 2 * 60 * 60;
const DEVICE_REFRESH_TOKEN_TTL_SECONDS = 1000 * 365 * 24 * 60 * 60;

const registerDeviceSchema = z.object({
  email: z.email(),
  password_auth: z.base64(),
  name: z.string().min(1),
  platform: z.string().min(1),
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

  const { recipients, ownerFlagOverrides } = await listBatchAccessRecipientsForOwner(
    c.env.DB,
    device.owner,
  );
  const [hashToken, featureFlags] = await Promise.all([
    generateToken('device', device.id, c.env.JWT_PRIVATE_KEY, HASH_TOKEN_TTL_SECONDS),
    // API-061: a device gets its owner's flags, so all their devices agree.
    enabledFeatureFlags(c.env.DB, device.owner, ownerFlagOverrides),
  ]);

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
    feature_flags: featureFlags,
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
    return unverifiedLoginResponse(c, result.user);
  }

  const owner = result.user.id;
  const id = uuidv4();

  await fillNewsletterLocation(c.env, result.user.email, requestLocation(c.req.raw));

  await createDevice(c.env.DB, { id, owner, name, platform });
  const refreshToken = await createDeviceSession(c, id);

  const settings = await buildDeviceSettings(c, { id, owner, name, platform });
  if (!settings) {
    return c.json({ error: 'Hash server not configured' }, 500);
  }

  return c.json({ token: refreshToken, settings }, 200);
});

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

    const createdAt = Date.now();
    // API-038: the times come from the device clock, and a batch is uploaded after the
    // period it covers, so an end_time past now means the clock runs ahead. Clamping it
    // keeps end_time <= created_at, which listDigestActivity relies on.
    const endTime = Math.min(end_time, createdAt);
    const startTime = Math.min(start_time, endTime);
    if (endTime !== end_time) {
      console.warn(
        `[batch] device ${device.id} sent end_time ${end_time - createdAt} ms in the future; clamped`,
      );
    }
    if (startTime !== start_time) {
      console.warn(
        `[batch] device ${device.id} sent start_time ${start_time - endTime} ms after end_time; clamped`,
      );
    }

    const hashState = await hashGet(c.env, device.id);
    const endHash = hashState.hash;
    // end_hash is stored as its 32 raw bytes, and decodeHex would silently truncate a
    // malformed value, so hold the hash server to its 64-hex-character contract.
    if (!/^[0-9a-f]{64}$/i.test(endHash)) {
      console.error(`[batch] hash server returned a malformed hash for device ${device.id}`);
      return c.json({ error: 'Hash server returned an invalid hash' }, 502);
    }
    const batchId = uuidv4();
    const stored = { id: batchId, user_id: device.owner, version: CURRENT_API_VERSION };
    const { key, url } = batchObject(c.env, stored);

    await putObject(c.env, key, await file.arrayBuffer(), 'application/octet-stream');
    const inserted = await createBatch(c.env.DB, {
      ...stored,
      device_id: device.id,
      start_time: startTime,
      end_time: endTime,
      end_hash: decodeHex(endHash),
      access_keys: encodeAccessKeys(access_keys),
      high_risk_count: event_counts.high,
      medium_risk_count: event_counts.medium,
      created_at: createdAt,
    });
    if (inserted.meta.changes === 0) {
      // The device was deleted while this upload was in flight.
      await deleteObject(c.env, key);
      return c.json({ error: 'Not found' }, 404);
    }
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

    return c.json(
      { id: batchId, start_time: startTime, end_time: endTime, end_hash: endHash, url, settings },
      200,
    );
  },
);

export default deviceOnly;
