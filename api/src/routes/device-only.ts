import { Context, Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { z } from 'zod';
import { authenticateWebSession, authenticateDeviceSession } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import {
  createBatch,
  createDevice,
  createSessionRecord,
  deleteDeviceSessionsByDeviceId,
  getHashState,
  findDeviceById,
  listBatchAccessRecipientsForOwner,
  markDeviceDeleted,
  resetHashState as resetStoredHashState,
} from '../lib/db';
import { encodeBase64, encodeHex } from '../lib/encoding';
import { generateToken } from '../lib/jwt';
import { putObject } from '../lib/r2';
import { notifyPartnersAboutRiskLog, riskToSeverity } from '../lib/tamper';
import { generateOpaqueToken, hashOpaqueToken } from '../lib/tokens';
import { Env, Variables } from '../types/bindings';

const deviceOnly = new Hono<{ Bindings: Env; Variables: Variables }>();
const ZERO_STATE = new Uint8Array(32);

function getAppUrl(c: Context<{ Bindings: Env; Variables: Variables }>) {
  return c.env.APP_URL;
}
const HASH_TOKEN_TTL_SECONDS = 60 * 60;
const DEVICE_REFRESH_TOKEN_TTL_SECONDS = 1000 * 365 * 24 * 60 * 60;

const createDeviceSchema = z.object({
  name: z.string().min(1),
  platform: z.string().min(1),
});

const uploadBatchSchema = z.object({
  start_time: z.coerce.number().int().nonnegative(),
  end_time: z.coerce.number().int().nonnegative(),
  access_keys: z.string().min(1),
  high_risk_count: z.coerce.number().int().nonnegative().optional().default(0),
  medium_risk_count: z.coerce.number().int().nonnegative().optional().default(0),
  file: z
    .instanceof(File)
    .refine((file) => file.size > 0, { message: 'File is empty' })
    .refine((file) => file.size <= 100 * 1024 * 1024, { message: 'File exceeds 100MB limit' }),
});

const accessKeyEntrySchema = z.object({
  user_id: z.uuid(),
  hpke_key: z.base64(),
});

const accessKeysSchema = z.object({
  keys: z.array(accessKeyEntrySchema).min(1),
});

const notifySchema = z.object({
  ts: z.number().int().nonnegative(),
  type: z.string().min(1),
  risk: z.number().min(0).max(1),
  title: z.string().optional(),
  details: z.string().optional(),
});

function parseAccessKeysPayload(raw: string) {
  const parsed = JSON.parse(raw) as unknown;
  const payload = accessKeysSchema.parse(parsed);
  const seen = new Set<string>();

  for (const key of payload.keys) {
    if (seen.has(key.user_id)) {
      throw new Error('access_keys contains duplicate user_id entries');
    }
    seen.add(key.user_id);
  }

  return payload;
}

async function readHashState(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  deviceId: string,
) {
  const state = await getHashState(c.env.DB, deviceId);
  return state ? state.state : ZERO_STATE.buffer;
}

async function resetHashState(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  device: { id: string; owner: string },
) {
  await resetStoredHashState(c.env.DB, device.id, Date.now());
}

async function createDeviceSession(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  deviceId: string,
) {
  const refreshToken = generateOpaqueToken();
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
 * POST /d/device - Register a device using the authenticated web session cookie.
 */
deviceOnly.post(
  '/device',
  authenticateWebSession(),
  validateZ('json', createDeviceSchema),
  async (c) => {
    const { name, platform } = c.req.valid('json');
    const owner = c.get('sub');
    const id = uuidv4();

    await createDevice(c.env.DB, { id, owner, name, platform });

    const refreshToken = await createDeviceSession(c, id);

    return c.json({ id, refresh_token: refreshToken }, 201);
  },
);

/**
 * GET /d/device - Get device settings for the authenticated device.
 */
deviceOnly.get('/device', authenticateDeviceSession(), async (c) => {
  const device = await findDeviceById(c.env.DB, c.get('sub'));

  if (!device) {
    return c.json({ error: 'Not found' }, 404);
  }

  const hashBaseUrl = c.env.HASH_SERVER_URL?.trim();
  if (!hashBaseUrl) {
    return c.json({ error: 'Hash server not configured' }, 500);
  }

  const recipients = await listBatchAccessRecipientsForOwner(c.env.DB, device.owner);

  return c.json({
    id: device.id,
    name: device.name,
    platform: device.platform,
    wrapping_keys: recipients.map((recipient) => ({
      user_id: recipient.id,
      pub_key: encodeBase64(recipient.pub_key!),
    })),
    hash_base_url: hashBaseUrl,
  });
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

  const hashServerUrl = c.env.HASH_SERVER_URL?.trim();
  try {
    if (hashServerUrl?.endsWith('/api')) {
      await resetStoredHashState(c.env.DB, deviceId, now);
    } else if (hashServerUrl) {
      const token = await generateToken('server', deviceId, c.env.JWT_PRIVATE_KEY, 60);
      await fetch(`${hashServerUrl}/hash`, {
        method: 'DELETE',
        headers: { Authorization: `Bearer ${token}` },
      });
    }
  } catch {
    // Non-fatal — logout/soft-delete already committed; hash state is best-effort.
  }

  return c.body(null, 204);
});

/**
 * POST /d/token - Exchange the opaque device refresh token for a short-lived hash-server JWT.
 */
deviceOnly.post('/token', authenticateDeviceSession(), async (c) => {
  const deviceId = c.get('sub');
  const device = await findDeviceById(c.env.DB, deviceId);
  if (!device) {
    return c.json({ error: 'Not found' }, 404);
  }

  const hashToken = await generateToken(
    'hash-server',
    deviceId,
    c.env.JWT_PRIVATE_KEY,
    HASH_TOKEN_TTL_SECONDS,
  );

  return c.json({ hash_token: hashToken });
});

/**
 * POST /d/batch - Upload an encrypted batch blob for the authenticated device.
 */
deviceOnly.post(
  '/batch',
  authenticateDeviceSession(),
  validateZ('form', uploadBatchSchema),
  async (c) => {
    const device = await findDeviceById(c.env.DB, c.get('sub'));

    if (!device) {
      return c.json({ error: 'Not found' }, 404);
    }

    const { start_time, end_time, access_keys, high_risk_count, medium_risk_count, file } =
      c.req.valid('form');

    const hashState = await readHashState(c, device.id);
    const endHash = encodeHex(hashState);
    const batchId = uuidv4();
    const key = `user/${device.owner}/batches/${batchId}.enc`;
    const url = `${c.env.R2_URL}/${key}`;
    const createdAt = Date.now();
    let parsedAccessKeys: z.infer<typeof accessKeysSchema>;

    try {
      parsedAccessKeys = parseAccessKeysPayload(access_keys);
    } catch (error) {
      return c.json(
        {
          error: 'Bad Request',
          details: { errors: [error instanceof Error ? error.message : 'Invalid access_keys'] },
        },
        400,
      );
    }

    await putObject(c.env, key, await file.arrayBuffer(), 'application/octet-stream');
    await createBatch(c.env.DB, {
      id: batchId,
      user_id: device.owner,
      device_id: device.id,
      url,
      start_time,
      end_time,
      end_hash: endHash,
      access_keys: JSON.stringify(parsedAccessKeys),
      high_risk_count,
      medium_risk_count,
      created_at: createdAt,
    });
    await resetHashState(c, device);

    return c.json({ id: batchId, start_time, end_time, end_hash: endHash, url }, 201);
  },
);

/**
 * POST /d/notify - Send an alert email for a high-risk event.
 *
 * The event itself is uploaded (end-to-end encrypted) via POST /d/batch; this
 * endpoint only carries the minimal metadata needed to render the notification
 * email and is not persisted. Replaces the removed POST /d/log endpoint.
 */
deviceOnly.post(
  '/notify',
  authenticateDeviceSession(),
  validateZ('json', notifySchema),
  async (c) => {
    const device = await findDeviceById(c.env.DB, c.get('sub'));

    if (!device) {
      return c.json({ error: 'Not found' }, 404);
    }

    const notification = c.req.valid('json');
    const providedTitle = notification.title?.trim();
    const providedDetails = notification.details?.trim();
    await notifyPartnersAboutRiskLog(c.env.DB, c.env, {
      logId: uuidv4(),
      appUrl: getAppUrl(c),
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

    return c.json({ ok: true }, 202);
  },
);

export default deviceOnly;
