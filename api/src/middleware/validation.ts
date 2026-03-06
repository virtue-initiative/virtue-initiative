import { zValidator } from '@hono/zod-validator';
import { ValidationTargets } from 'hono';
import { z } from 'zod';

export const validateZ = <T extends z.ZodTypeAny, Target extends keyof ValidationTargets>(
  target: Target,
  schema: T,
) =>
  zValidator(target, schema, (result, c) => {
    if (!result.success) {
      return c.json({ error: 'Bad Request', details: z.treeifyError(result.error) }, 400);
    }
  });
