import { Context, Next } from 'hono';
import { Env, Variables } from '../types/bindings';
import { verifyJWT } from '../lib/jwt';

/**
 * Middleware to authenticate JWT access tokens
 */
export async function authenticate(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  next: Next,
) {
  const authHeader = c.req.header('Authorization');

  if (!authHeader || !authHeader.startsWith('Bearer ')) {
    return c.json({ error: 'Unauthorized' }, 401);
  }

  const token = authHeader.substring(7);

  try {
    const payload = await verifyJWT(token, c.env.JWT_SECRET);

    if (payload.type !== 'access') {
      return c.json({ error: 'Invalid token type' }, 401);
    }

    // Store user ID in context for route handlers
    c.set('userId', payload.sub);

    await next();
  } catch (error) {
    return c.json({ error: 'Invalid or expired token' }, 401);
  }
}
