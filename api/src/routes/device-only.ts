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
  deleteBatch,
  deleteDeviceSessionsByDeviceId,
  findDeviceById,
  findUserById,
  listAcceptedNotificationTargetsForUser,
  listBatchAccessRecipientsForOwner,
  markDeviceDeleted,
  setDeviceAppVersion,
} from '../lib/db';
import { encodeAccessKeys } from '../lib/access-keys';
import { batchObject } from '../lib/batch-url';
import { sendEmail } from '../lib/email';
import { renderDeviceLoggedOutTemplate } from '../lib/email/templates';
import { hashGet, hashReset, hashResetAt } from '../lib/hash-server';
import { decodeHex, encodeBase64 } from '../lib/encoding';
import { announcementForDevice, validVersion } from '../lib/announcements';
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

// HASH-006: seq is a u32 on the hash server, and 0 is never accepted.
const hashSeqSchema = z.number().int().positive().max(0xffff_ffff);

const batchMetadataSchema = z
  .object({
    start_time: z.number().int().nonnegative(),
    end_time: z.number().int().nonnegative(),
    start_seq: hashSeqSchema.optional(),
    end_seq: hashSeqSchema.optional(),
    access_keys: z.record(z.uuid(), z.base64()),
    event_counts: eventCountsSchema,
    notifications: z.array(notifyEntrySchema).optional().default([]),
  })
  .refine((m) => (m.start_seq === undefined) === (m.end_seq === undefined), {
    message: 'start_seq and end_seq must be sent together',
  })
  .refine((m) => m.start_seq === undefined || m.start_seq <= m.end_seq!, {
    message: 'start_seq must not be greater than end_seq',
  });

const uploadBatchSchema = z.object({
  metadata: jsonField(batchMetadataSchema, 'metadata'),
  file: z
    .instanceof(File)
    .refine((file) => file.size > 0, { message: 'File is empty' })
    .refine((file) => file.size <= 100 * 1024 * 1024, { message: 'File exceeds 100MB limit' }),
});

// API-068: the client's version from X-Virtue-Version, or null when it is
// missing or not valid. Stored on the device and used to pick its announcement.
function clientVersion(c: Context<{ Bindings: Env; Variables: Variables }>) {
  return validVersion(c.req.header('X-Virtue-Version'));
}

// API-036: store a changed version on the device. Only a change is written,
// and a failed write must never fail the settings fetch or the batch upload.
async function recordClientVersion(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  device: { id: string; app_version: string | null },
  version: string | null,
) {
  if (!version || version === device.app_version) return;
  try {
    await setDeviceAppVersion(c.env.DB, device.id, version);
  } catch (err) {
    console.error(`[device] failed to store version for device ${device.id}`, err);
  }
}

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
 * responses — the one canonical place a device's wrapping keys, a fresh hash-server
 * token (DeviceSettings.hash_token) and its announcement are assembled.
 */
async function buildDeviceSettings(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  device: {
    id: string;
    owner: string;
    name: string;
    platform: string;
    dns_filter_disabled: number;
  },
  version: string | null,
) {
  const hashBaseUrl = c.env.HASH_SERVER_URL?.trim();
  if (!hashBaseUrl) {
    return null; // caller returns the existing 500
  }

  const { recipients, ownerFlagOverrides, ownerDnsFilterDomain } =
    await listBatchAccessRecipientsForOwner(c.env.DB, device.owner);
  const [hashToken, featureFlags, announcement] = await Promise.all([
    generateToken('device', device.id, c.env.JWT_PRIVATE_KEY, HASH_TOKEN_TTL_SECONDS),
    // API-061: a device gets its owner's flags, so all their devices agree.
    enabledFeatureFlags(c.env.DB, device.owner, ownerFlagOverrides),
    // API-068: the device's version comes from the request, not the database.
    announcementForDevice(c.env.DB, { platform: device.platform, version }),
  ]);

  return {
    id: device.id,
    name: device.name,
    platform: device.platform,
    // CORE-029: the owner's domain, unless they turned the check off for this device.
    dns_filter_domain: device.dns_filter_disabled ? null : ownerDnsFilterDomain,
    wrapping_keys: recipients.map((recipient) => ({
      user_id: recipient.id,
      pub_key: encodeBase64(recipient.pub_key!),
    })),
    hash_base_url: hashBaseUrl,
    hash_token: hashToken,
    feature_flags: featureFlags,
    announcement,
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

  const version = clientVersion(c);
  await createDevice(c.env.DB, { id, owner, name, platform, app_version: version });
  const refreshToken = await createDeviceSession(c, id);

  const settings = await buildDeviceSettings(
    c,
    { id, owner, name, platform, dns_filter_disabled: 0 },
    version,
  );
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

  // API-036: a client fetches its settings when it starts, so an update
  // usually shows up here.
  const version = clientVersion(c);
  await recordClientVersion(c, device, version);

  const settings = await buildDeviceSettings(c, device, version);
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

const hashChainMoved = {
  error: 'The device hashed more events after this batch was built',
  code: 'hash_chain_moved',
} as const;

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
    const { start_time, end_time, start_seq, end_seq, access_keys, event_counts, notifications } =
      metadata;

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
    if (start_time > end_time) {
      console.warn(
        `[batch] device ${device.id} sent start_time ${start_time - end_time} ms after end_time; clamped`,
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
    // API-038: with the batch's seqs, check it against the chain the hash server holds.
    const checkSeqs = start_seq !== undefined && end_seq !== undefined;
    if (checkSeqs) {
      const firstSeq = hashState.first_seq ?? 0;
      if (hashState.seq === 0 || (firstSeq !== 0 && firstSeq > start_seq)) {
        // An earlier attempt stored this batch's first events and reset the chain,
        // and the device never got that response.
        return c.json(
          {
            error: 'This batch begins with events that were already stored',
            code: 'batch_already_stored',
            first_seq: firstSeq,
            seq: hashState.seq,
          },
          409,
        );
      }
      if (hashState.seq > end_seq) {
        return c.json(hashChainMoved, 409);
      }
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
    if (!checkSeqs) {
      await hashReset(c.env, device.id);
    } else if (!(await hashResetAt(c.env, device.id, hashState.seq))) {
      // API-038: the device hashed more while this was stored, so end_hash no longer
      // ends the chain. Only a retry holding those events can be stored.
      await deleteBatch(c.env.DB, { ...stored, created_at: createdAt });
      await deleteObject(c.env, key);
      return c.json(hashChainMoved, 409);
    }

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

    // API-036: a client that started offline reports an update here.
    const version = clientVersion(c);
    await recordClientVersion(c, device, version);

    const settings = await buildDeviceSettings(c, device, version);
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
