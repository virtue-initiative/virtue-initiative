import { Context, Next } from 'hono';
import { Env, Variables } from '../types/bindings';

/** Must run after an auth middleware that calls `c.set('sub', ...)`. */
export function rateLimitByDevice() {
  return async function rateLimitMiddleware(
    c: Context<{ Bindings: Env; Variables: Variables }>,
    next: Next,
  ) {
    const { success } = await c.env.RATE_LIMITER.limit({ key: c.get('sub') });
    if (!success) {
      return c.json({ error: 'Too many requests' }, 429);
    }
    await next();
  };
}

/** For routes that may be called without authentication. */
export function rateLimitByIp() {
  return async function rateLimitMiddleware(
    c: Context<{ Bindings: Env; Variables: Variables }>,
    next: Next,
  ) {
    const ip = c.req.header('CF-Connecting-IP') ?? 'unknown';
    const { success } = await c.env.RATE_LIMITER.limit({ key: `ip:${ip}` });
    if (!success) {
      return c.json({ error: 'Too many requests' }, 429);
    }
    await next();
  };
}
