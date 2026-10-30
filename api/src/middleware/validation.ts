import { zValidator } from '@hono/zod-validator';
import { MiddlewareHandler, ValidationTargets } from 'hono';
import { z } from 'zod';
import { isUuid } from '../lib/db';

/** API-004: an `:id` that is not a UUID names nothing. Goes after the auth middleware. */
export const requireUuidParam = (): MiddlewareHandler => async (c, next) => {
  if (!isUuid(c.req.param('id') ?? '')) {
    return c.json({ error: 'Not found' }, 404);
  }
  await next();
};

export const validateZ = <T extends z.ZodTypeAny, Target extends keyof ValidationTargets>(
  target: Target,
  schema: T,
) =>
  zValidator(target, schema, (result, c) => {
    if (!result.success) {
      return c.json({ error: 'Invalid request data', details: z.treeifyError(result.error) }, 400);
    }
  });
