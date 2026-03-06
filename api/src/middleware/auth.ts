import { Context, Next } from 'hono';
import { Env, Variables } from '../types/bindings';
import { JWTType, verifyJWT } from '../lib/jwt';

export function authenticate(type: JWTType) {
  return async function authMiddleware(
    c: Context<{ Bindings: Env; Variables: Variables }>,
    next: Next,
  ) {
    const authHeader = c.req.header('Authorization');

    if (!authHeader?.startsWith('Bearer ')) {
      return c.json({ error: 'Unauthorized' }, 401);
    }

    try {
      const payload = await verifyJWT(authHeader.slice(7), c.env.JWT_SECRET);

      if (payload.type !== type) {
        return c.json({ error: 'Unauthorized', details: { reason: 'Invalid token type' } }, 401);
      }

      c.set('sub', payload.sub);
      await next();
    } catch (error) {
      return c.json({ error: 'Unauthorized', details: { reason: 'Invalid or expired token' } }, 401);
    }
  };
}
