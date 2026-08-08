import { Hono } from 'hono';
import { authenticateWebSession } from '../middleware/auth';
import { buildDeviceViews, buildPartnerRelationships, buildUserView } from '../lib/views';
import { Env, Variables } from '../types/bindings';
import type { Updates } from '../../../shared-web/types';

const updates = new Hono<{ Bindings: Env; Variables: Variables }>();

updates.get('/updates', authenticateWebSession(), async (c) => {
  const userId = c.get('sub');
  const user = await buildUserView(c.env.DB, userId);
  if (!user) return c.json({ error: 'User account not found' }, 404);

  const [devices, partners] = await Promise.all([
    buildDeviceViews(c.env, userId),
    buildPartnerRelationships(c.env.DB, userId),
  ]);
  return c.json<Updates>({ user, devices, partners });
});

export default updates;
