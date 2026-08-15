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
  listBatchAccessRecipientsForOwner,
  markDeviceDeleted,
} from '../lib/db';
import { hashGet, hashReset } from '../lib/hash-server';
import { encodeBase64 } from '../lib/encoding';
import { generateToken } from '../lib/jwt';
import { putObject } from '../lib/r2';
import { notifyPartnersAboutRiskLog, riskToSeverity } from '../lib/tamper';
import { generateOpaqueToken, hashOpaqueToken } from '../lib/tokens';
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

// Parses a multipart form field that carries a JSON string (e.g. `metadata`), surfacing
// both malformed JSON and schema violations through the same zod issue-reporting path
// as every other validateZ() failure.
function jsonField<Schema extends z.ZodTypeAny>(schema: Schema, label: string) {
  return z.string().transform((raw, ctx) => {
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch {
      ctx.addIssue({ code: 'custom', message: `${label} must be valid JSON` });
      return z.NEVER;
    }

    const result = schema.safeParse(parsed);
    if (!result.success) {
      for (const issue of result.error.issues) {
        ctx.addIssue({ code: 'custom', message: issue.message, path: issue.path });
      }
      return z.NEVER;
    }

    return result.data as z.infer<Schema>;
  });
}

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
 */
deviceOnly.post('/logout', authenticateDeviceSession(), async (c) => {
  const deviceId = c.get('sub');
  const now = Date.now();

  await deleteDeviceSessionsByDeviceId(c.env.DB, deviceId);
  await markDeviceDeleted(c.env.DB, deviceId, now);

  try {
    await hashReset(c.env, deviceId);
  } catch {
    // Non-fatal — logout/soft-delete already committed; hash state is best-effort.
  }

  return c.body(null, 204);
});

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
