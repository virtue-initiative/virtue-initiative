import z from 'zod';
import { Hono } from 'hono';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { findDevice } from '../lib/db';

const hashServerSchema = z.object({
  deviceId: z.string().min(1),
});

const hashServer = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * GET /hash-server?deviceId=<id> — Returns the base URL for hash upload endpoints.
 * Defaults to the current API origin if HASH_SERVER_URL is not configured.
 */
hashServer.get('/', authenticate, async (c) => {
  const parsed = hashServerSchema.safeParse(c.req.query());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const userId = c.get('userId');
  const { deviceId } = parsed.data;

  const device = await findDevice(c.env.DB, deviceId, userId);
  if (!device) return c.json({ error: 'Device not found' }, 404);

  const url = c.env.HASH_SERVER_URL ?? new URL(c.req.url).origin;

  return c.json({ url });
});

export default hashServer;
