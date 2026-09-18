import { Context, Next } from 'hono';
import { Env, Variables } from '../types/bindings';
import { isAdmin } from '../lib/db';

// api/SPEC.md API-051: runs after authenticateWebSession(); non-admins get 403.
export function requireAdmin() {
  return async function adminMiddleware(
    c: Context<{ Bindings: Env; Variables: Variables }>,
    next: Next,
  ) {
    if (!(await isAdmin(c.env.DB, c.get('sub')))) {
      return c.json({ error: 'Forbidden' }, 403);
    }
    await next();
  };
}
