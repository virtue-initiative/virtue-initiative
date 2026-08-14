import { Context, Next } from 'hono';
import { getCookie } from 'hono/cookie';
import { Env, Variables } from '../types/bindings';
import { findSessionByRefreshTokenHash } from '../lib/db';
import { assertTokenPurpose, hashOpaqueToken } from '../lib/tokens';

export function authenticateWebSession() {
  return async function webSessionMiddleware(
    c: Context<{ Bindings: Env; Variables: Variables }>,
    next: Next,
  ) {
    const cookieToken = getCookie(c, 'refresh_token');
    const authHeader = c.req.header('Authorization');
    const bearerToken = authHeader?.startsWith('Bearer ') ? authHeader.slice(7) : undefined;
    const refreshToken = cookieToken ?? bearerToken;
    if (!refreshToken) {
      return c.json({ error: 'Unauthorized' }, 401);
    }

    try {
      assertTokenPurpose(refreshToken, 'web_session');
    } catch {
      return c.json({ error: 'Unauthorized' }, 401);
    }

    const session = await findSessionByRefreshTokenHash(
      c.env.DB,
      hashOpaqueToken(refreshToken),
      'web',
    );

    if (!session || !session.user_id || session.expires_at < Date.now()) {
      return c.json({ error: 'Unauthorized' }, 401);
    }

    c.set('sub', session.user_id);
    await next();
  };
}

export function authenticateDeviceSession() {
  return async function deviceSessionMiddleware(
    c: Context<{ Bindings: Env; Variables: Variables }>,
    next: Next,
  ) {
    const authHeader = c.req.header('Authorization');
    if (!authHeader?.startsWith('Bearer ')) {
      return c.json({ error: 'Unauthorized' }, 401);
    }

    const token = authHeader.slice(7);
    try {
      assertTokenPurpose(token, 'device_session');
    } catch {
      return c.json({ error: 'Unauthorized' }, 401);
    }

    const session = await findSessionByRefreshTokenHash(c.env.DB, hashOpaqueToken(token), 'device');

    if (!session || !session.device_id || session.expires_at < Date.now()) {
      return c.json({ error: 'Unauthorized' }, 401);
    }

    c.set('sub', session.device_id);
    await next();
  };
}
