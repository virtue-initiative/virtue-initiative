import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { z } from 'zod';
import { authenticate } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import {
  createBatch,
  createDevice,
  createDeviceLog,
  findDeviceById,
  findUserById,
} from '../lib/db';
import { encodeBase64, encodeHex } from '../lib/encoding';
import { generateToken, verifyJWT } from '../lib/jwt';
import { putObject } from '../lib/r2';
import { Env, Variables } from '../types/bindings';

const deviceOnly = new Hono<{ Bindings: Env; Variables: Variables }>();
const DEVICE_ACCESS_TOKEN_TTL_SECONDS = 7 * 24 * 60 * 60;
const DEVICE_REFRESH_TOKEN_TTL_SECONDS = 365 * 24 * 60 * 60;

const createDeviceSchema = z.object({
  name: z.string().min(1),
  platform: z.string().min(1),
});

const deviceTokenSchema = z.object({
  refresh_token: z.string().min(1),
});

const uploadBatchSchema = z.object({
  start: z.coerce.number().int().nonnegative(),
  end: z.coerce.number().int().nonnegative(),
  file: z
    .instanceof(File)
    .refine((file) => file.size > 0, { message: 'File is empty' })
    .refine((file) => file.size <= 100 * 1024 * 1024, { message: 'File exceeds 100MB limit' }),
});

const deviceLogSchema = z.object({
  ts: z.number().int().nonnegative(),
  type: z.string().min(1),
  data: z.record(z.string(), z.unknown()).optional().default({}),
});

function getHashBaseUrl(requestUrl: string, configuredUrl?: string) {
  return configuredUrl ?? new URL(requestUrl).origin;
}

async function readHashState(hashBaseUrl: string, authorization: string) {
  const response = await fetch(`${hashBaseUrl}/hash`, {
    headers: { Authorization: authorization },
  });

  if (!response.ok) {
    throw new Error(`Failed to read hash state: ${response.status} ${await response.text()}`);
  }

  return response.arrayBuffer();
}

async function resetHashState(hashBaseUrl: string, serverToken: string) {
  const response = await fetch(`${hashBaseUrl}/hash`, {
    method: 'DELETE',
    headers: { Authorization: `Bearer ${serverToken}` },
  });

  if (!response.ok) {
    throw new Error(`Failed to reset hash state: ${response.status} ${await response.text()}`);
  }
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
      generateToken('device-refresh', id, c.env.JWT_SECRET, DEVICE_REFRESH_TOKEN_TTL_SECONDS),
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

  const user = await findUserById(c.env.DB, device.owner);

  return c.json({
    id: device.id,
    name: device.name,
    platform: device.platform,
    enabled: device.enabled === 1,
    ...(user?.e2ee_key ? { e2ee_key: encodeBase64(user.e2ee_key) } : {}),
    hash_base_url: getHashBaseUrl(c.req.url, c.env.HASH_SERVER_URL),
  });
});

/**
 * POST /d/token - Exchange a device refresh token for a new device access token.
 */
deviceOnly.post('/token', validateZ('json', deviceTokenSchema), async (c) => {
  const { refresh_token } = c.req.valid('json');

  try {
    const payload = await verifyJWT(refresh_token, c.env.JWT_SECRET);

    if (payload.type !== 'device-refresh') {
      return c.json({ error: 'Unauthorized' }, 401);
    }

    const device = await findDeviceById(c.env.DB, payload.sub);
    if (!device) {
      return c.json({ error: 'Not found' }, 404);
    }

    const accessToken = await generateToken(
      'device-access',
      payload.sub,
      c.env.JWT_SECRET,
      DEVICE_ACCESS_TOKEN_TTL_SECONDS,
    );

    return c.json({ access_token: accessToken });
  } catch (error) {
    return c.json({ error: 'Unauthorized' }, 401);
  }
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

    const { start, end, file } = c.req.valid('form');
    const authorization = c.req.header('Authorization');

    if (!authorization) {
      return c.json({ error: 'Unauthorized' }, 401);
    }

    const hashBaseUrl = getHashBaseUrl(c.req.url, c.env.HASH_SERVER_URL);
    const hashState = await readHashState(hashBaseUrl, authorization);
    const endHash = encodeHex(hashState);
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
      start,
      end,
      end_hash: endHash,
      created_at: createdAt,
    });
    await resetHashState(
      hashBaseUrl,
      await generateToken('server', device.id, c.env.JWT_SECRET, 60),
    );

    return c.json({ id: batchId, start, end, end_hash: endHash, url }, 201);
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

    await createDeviceLog(c.env.DB, {
      id: uuidv4(),
      user_id: device.owner,
      device_id: device.id,
      ts: log.ts,
      type: log.type,
      data: JSON.stringify(log.data),
      created_at: Date.now(),
    });

    return c.json(log, 201);
  },
);

export default deviceOnly;
