import { Context, Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { z } from 'zod';
import { authenticate } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import { getRequestApiBaseUrl } from '../lib/base-path';
import {
  createBatch,
  createDevice,
  createDeviceLog,
  createSessionRecord,
  getHashState,
  findDeviceById,
  findSessionByRefreshTokenHash,
  deleteSessionByRefreshTokenHash,
  listBatchAccessRecipientsForOwner,
  resetHashState as resetStoredHashState,
} from '../lib/db';
import { encodeBase64, encodeHex } from '../lib/encoding';
import { generateToken } from '../lib/jwt';
import { putObject } from '../lib/r2';
import { notifyPartnersAboutRiskLog, riskToSeverity } from '../lib/tamper';
import { generateOpaqueToken, hashOpaqueToken } from '../lib/tokens';
import { Env, Variables } from '../types/bindings';

const deviceOnly = new Hono<{ Bindings: Env; Variables: Variables }>();
const LOCAL_WEB_URL = 'http://localhost:5173';
const ZERO_STATE = new Uint8Array(32);

function getAppUrl(c: Context<{ Bindings: Env; Variables: Variables }>) {
  const requestUrl = new URL(c.req.url);
  if (requestUrl.hostname === 'localhost' || requestUrl.hostname === '127.0.0.1') {
    return LOCAL_WEB_URL;
  }

  return c.env.APP_URL;
}
const DEVICE_ACCESS_TOKEN_TTL_SECONDS = 7 * 24 * 60 * 60;
const DEVICE_REFRESH_TOKEN_TTL_SECONDS = 1000 * 365 * 24 * 60 * 60;

const createDeviceSchema = z.object({
  name: z.string().min(1),
  platform: z.string().min(1),
});

const deviceTokenSchema = z.object({
  refresh_token: z.string().min(1),
});

const uploadBatchSchema = z.object({
  start_time: z.coerce.number().int().nonnegative(),
  end_time: z.coerce.number().int().nonnegative(),
  access_keys: z.string().min(1),
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

const deviceLogSchema = z.object({
  ts: z.number().int().nonnegative(),
  type: z.string().min(1),
  risk: z.number().min(0).max(1).optional(),
  data: z.record(z.string(), z.unknown()).optional().default({}),
});

function getConfiguredHashBaseUrl(env: Env) {
  const trimmed = env.HASH_SERVER_URL?.trim();
  return trimmed ? trimmed : null;
}

function getHashBaseUrl(requestUrl: string, env: Env) {
  return getConfiguredHashBaseUrl(env) ?? getRequestApiBaseUrl(requestUrl, env.API_BASE_PATH);
}

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
  authorization: string,
  hashBaseUrl: string | null,
) {
  if (!hashBaseUrl) {
    const state = await getHashState(c.env.DB, deviceId);
    return state ? state.state : ZERO_STATE.buffer;
  }

  const response = await fetch(`${hashBaseUrl}/hash`, {
    headers: { Authorization: authorization },
  });

  if (!response.ok) {
    throw new Error(`Failed to read hash state: ${response.status} ${await response.text()}`);
  }

  return response.arrayBuffer();
}

async function resetHashState(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  device: { id: string; owner: string },
  hashBaseUrl: string | null,
  serverToken: string,
) {
  if (!hashBaseUrl) {
    await resetStoredHashState(c.env.DB, device.id, Date.now());
    return;
  }

  const response = await fetch(`${hashBaseUrl}/hash`, {
    method: 'DELETE',
    headers: { Authorization: `Bearer ${serverToken}` },
  });

  if (!response.ok) {
    throw new Error(`Failed to reset hash state: ${response.status} ${await response.text()}`);
  }
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
 * POST /d/device - Register a device using a user access token.
 */
deviceOnly.post(
  '/device',
  authenticate('access'),
  validateZ('json', createDeviceSchema),
  async (c) => {
    const { name, platform } = c.req.valid('json');
    const owner = c.get('sub');
    const id = uuidv4();

    await createDevice(c.env.DB, { id, owner, name, platform });

    const [accessToken, refreshToken] = await Promise.all([
      generateToken('device-access', id, c.env.JWT_SECRET, DEVICE_ACCESS_TOKEN_TTL_SECONDS),
      createDeviceSession(c, id),
    ]);

    return c.json({ id, access_token: accessToken, refresh_token: refreshToken }, 201);
  },
);

/**
 * GET /d/device - Get device settings for the authenticated device.
 */
deviceOnly.get('/device', authenticate('device-access'), async (c) => {
  const device = await findDeviceById(c.env.DB, c.get('sub'));

  if (!device) {
    return c.json({ error: 'Not found' }, 404);
  }

  const recipients = await listBatchAccessRecipientsForOwner(c.env.DB, device.owner);
  const owner = recipients.find((recipient) => recipient.id === device.owner);

  return c.json({
    id: device.id,
    name: device.name,
    platform: device.platform,
    enabled: device.enabled === 1,
    ...(owner?.pub_key
      ? {
          owner: {
            user_id: owner.id,
            pub_key: encodeBase64(owner.pub_key),
          },
        }
      : {}),
    partners: recipients
      .filter((recipient) => recipient.id !== device.owner)
      .map((recipient) => ({
        user_id: recipient.id,
        pub_key: encodeBase64(recipient.pub_key!),
      })),
    hash_base_url: getHashBaseUrl(c.req.url, c.env),
  });
});

/**
 * POST /d/token - Exchange a device refresh token for a new device access token.
 */
deviceOnly.post('/token', validateZ('json', deviceTokenSchema), async (c) => {
  const { refresh_token } = c.req.valid('json');

  const session = await findSessionByRefreshTokenHash(
    c.env.DB,
    hashOpaqueToken(refresh_token),
    'device',
  );

  if (!session || !session.device_id) {
    return c.json({ error: 'Unauthorized' }, 401);
  }

  if (session.expires_at < Date.now()) {
    await deleteSessionByRefreshTokenHash(c.env.DB, session.refresh_token_hash, 'device');
    return c.json({ error: 'Unauthorized' }, 401);
  }

  const device = await findDeviceById(c.env.DB, session.device_id);
  if (!device) {
    return c.json({ error: 'Not found' }, 404);
  }

  const accessToken = await generateToken(
    'device-access',
    session.device_id,
    c.env.JWT_SECRET,
    DEVICE_ACCESS_TOKEN_TTL_SECONDS,
  );

  return c.json({ access_token: accessToken });
});

/**
 * POST /d/batch - Upload an encrypted batch blob for the authenticated device.
 */
deviceOnly.post(
  '/batch',
  authenticate('device-access'),
  validateZ('form', uploadBatchSchema),
  async (c) => {
    const device = await findDeviceById(c.env.DB, c.get('sub'));

    if (!device) {
      return c.json({ error: 'Not found' }, 404);
    }

    const { start_time, end_time, access_keys, file } = c.req.valid('form');
    const authorization = c.req.header('Authorization');

    if (!authorization) {
      return c.json({ error: 'Unauthorized' }, 401);
    }

    const configuredHashBaseUrl = getConfiguredHashBaseUrl(c.env);
    const hashState = await readHashState(c, device.id, authorization, configuredHashBaseUrl);
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
      created_at: createdAt,
    });
    await resetHashState(
      c,
      device,
      configuredHashBaseUrl,
      await generateToken('server', device.id, c.env.JWT_SECRET, 60),
    );

    return c.json({ id: batchId, start_time, end_time, end_hash: endHash, url }, 201);
  },
);

/**
 * POST /d/log - Submit a single non-batched log item.
 */
deviceOnly.post(
  '/log',
  authenticate('device-access'),
  validateZ('json', deviceLogSchema),
  async (c) => {
    const device = await findDeviceById(c.env.DB, c.get('sub'));

    if (!device) {
      return c.json({ error: 'Not found' }, 404);
    }

    const log = c.req.valid('json');
    const providedTitle = typeof log.data.title === 'string' ? log.data.title : undefined;
    const providedDetails = typeof log.data.details === 'string' ? log.data.details : undefined;
    const computedRisk = log.risk ?? null;
    const computedSeverity = riskToSeverity(computedRisk);
    const logId = uuidv4();

    await createDeviceLog(c.env.DB, {
      id: logId,
      user_id: device.owner,
      device_id: device.id,
      ts: log.ts,
      type: log.type,
      data: JSON.stringify(log.data),
      risk: computedRisk,
      created_at: Date.now(),
    });

    if (computedSeverity && computedRisk != null) {
      await notifyPartnersAboutRiskLog(c.env.DB, c.env, {
        logId,
        appUrl: getAppUrl(c),
        userId: device.owner,
        severity: computedSeverity,
        risk: computedRisk,
        title:
          providedTitle && providedTitle.trim().length > 0
            ? providedTitle
            : `Device reported ${log.type.replaceAll('_', ' ')}.`,
        details: providedDetails && providedDetails.trim().length > 0 ? providedDetails : null,
        happenedAt: log.ts,
      });
    }

    return c.json({ ...log, ...(computedRisk != null ? { risk: computedRisk } : {}) }, 201);
  },
);

export default deviceOnly;
