import { Hono } from 'hono';
import { z } from 'zod';
import { emailFrequencies } from '../lib/email-domain';
import { findUserById, updateUser } from '../lib/db';
import { authenticate } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import { Env, Variables } from '../types/bindings';

const notifications = new Hono<{ Bindings: Env; Variables: Variables }>();

const updatePreferenceSchema = z
  .object({
    email_frequency: z.enum(emailFrequencies).optional(),
  })
  .refine((data) => Object.keys(data).length > 0, { message: 'No fields to update' });

notifications.get('/notifications/preferences', authenticate('access'), async (c) => {
  const user = await findUserById(c.env.DB, c.get('sub'));
  if (!user) {
    return c.json({ error: 'Not found' }, 404);
  }

  return c.json({ email_frequency: user.email_frequency });
});

notifications.patch(
  '/notifications/preferences',
  authenticate('access'),
  validateZ('json', updatePreferenceSchema),
  async (c) => {
    const user = await findUserById(c.env.DB, c.get('sub'));
    if (!user) {
      return c.json({ error: 'Not found' }, 404);
    }

    await updateUser(c.env.DB, c.get('sub'), {
      email_frequency: c.req.valid('json').email_frequency,
    });

    return c.json({ email_frequency: c.req.valid('json').email_frequency ?? user.email_frequency });
  },
);

export default notifications;
