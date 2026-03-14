import { Hono } from 'hono';
import { z } from 'zod';
import {
  emailFrequencies,
  immediateTamperSeverities,
  normalizeImmediateTamperSeverity,
} from '../lib/email-domain';
import {
  listNotificationPreferencesForPartner,
  updatePartnerNotificationPreference,
} from '../lib/db';
import { authenticate } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import { Env, Variables } from '../types/bindings';

const notifications = new Hono<{ Bindings: Env; Variables: Variables }>();

const updatePreferenceSchema = z
  .object({
    email_frequency: z.enum(emailFrequencies).optional(),
    immediate_tamper_severity: z.enum(immediateTamperSeverities).optional(),
  })
  .refine((data) => Object.keys(data).length > 0, { message: 'No fields to update' });

notifications.get('/notifications/preferences', authenticate('access'), async (c) => {
  const preferences = await listNotificationPreferencesForPartner(c.env.DB, c.get('sub'));
  return c.json(
    preferences.map((preference) => ({
      partnership_id: preference.partnership_id,
      status: preference.status,
      monitored_user: {
        id: preference.owner_id,
        email: preference.owner_email,
        ...(preference.owner_name ? { name: preference.owner_name } : {}),
      },
      email_frequency: preference.email_frequency ?? 'daily',
      immediate_tamper_severity: normalizeImmediateTamperSeverity(
        preference.immediate_tamper_severity,
      ),
    })),
  );
});

notifications.patch(
  '/notifications/preferences/:id',
  authenticate('access'),
  validateZ('json', updatePreferenceSchema),
  async (c) => {
    const result = await updatePartnerNotificationPreference(c.env.DB, {
      partnership_id: c.req.param('id'),
      partner_user_id: c.get('sub'),
      updated_at: Date.now(),
      ...c.req.valid('json'),
    });

    if (!result) {
      return c.json({ error: 'Not found' }, 404);
    }

    return c.json({
      ...result,
      immediate_tamper_severity: normalizeImmediateTamperSeverity(result.immediate_tamper_severity),
    });
  },
);

export default notifications;
