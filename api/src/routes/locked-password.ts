import { Hono } from 'hono';
import { authenticateWebSession } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import {
  createLockedPassword,
  findOwnedLockedPassword,
  findUserById,
  listAcceptedNotificationTargetsForUser,
  listLockedPasswordsForOwner,
  markLockedPasswordAccessed,
  markLockedPasswordDeleted,
  restoreLockedPassword,
  deleteLockedPasswordById,
} from '../lib/db';
import { sendEmail } from '../lib/email';
import { renderLockedPasswordAccessedTemplate } from '../lib/email/templates';
import { Env, Variables } from '../types/bindings';
import { createLockedPasswordSchema } from '../../../shared-web/types';

const lockedPasswords = new Hono<{ Bindings: Env; Variables: Variables }>();

async function notifyWatchersAboutLockedPasswordAccess(
  db: D1Database,
  env: Env,
  ownerId: string,
  label: string,
) {
  const owner = await findUserById(db, ownerId);
  if (!owner) {
    return;
  }

  const targets = await listAcceptedNotificationTargetsForUser(db, ownerId);
  for (const target of targets) {
    if (target.settings.email_frequency === 'none') {
      continue;
    }

    const email = renderLockedPasswordAccessedTemplate({
      appName: env.APP_NAME,
      appUrl: env.APP_URL,
      ownerName: owner.name,
      ownerEmail: owner.email,
      label,
    });

    await sendEmail({
      env,
      db,
      kind: 'locked_password_accessed',
      recipient: target.watcher_email,
      recipientEmailVerified: target.watcher_email_verified,
      subject: email.subject,
      text: email.text,
      html: email.html,
      related_user_id: ownerId,
      related_partnership_id: target.partnership_id,
      metadata: { label },
    });
  }
}

lockedPasswords.post(
  '/',
  authenticateWebSession(),
  validateZ('json', createLockedPasswordSchema),
  async (c) => {
    const { label, wrapped_value } = c.req.valid('json');
    const id = crypto.randomUUID();

    await createLockedPassword(c.env.DB, {
      id,
      owner_id: c.get('sub'),
      label,
      wrapped_value,
      created_at: Date.now(),
    });

    return c.json({ id });
  },
);

lockedPasswords.get('/', authenticateWebSession(), async (c) => {
  const rows = await listLockedPasswordsForOwner(c.env.DB, c.get('sub'));

  return c.json(
    rows.map((row) => ({
      id: row.id,
      label: row.label,
      created_at: row.created_at,
      accessed_at: row.accessed_at,
      deleted_at: row.deleted_at,
    })),
  );
});

lockedPasswords.post('/:id/reveal', authenticateWebSession(), async (c) => {
  const id = c.req.param('id');
  const ownerId = c.get('sub');
  const entry = await findOwnedLockedPassword(c.env.DB, id, ownerId);

  if (!entry) {
    return c.json({ error: 'Not found' }, 404);
  }

  const wasAccessed = entry.accessed_at !== null;
  const accessedAt = entry.accessed_at ?? Date.now();

  if (!wasAccessed) {
    await markLockedPasswordAccessed(c.env.DB, id, accessedAt);
    await notifyWatchersAboutLockedPasswordAccess(c.env.DB, c.env, ownerId, entry.label);
  }

  return c.json({ wrapped_value: entry.wrapped_value, accessed_at: accessedAt });
});

lockedPasswords.delete('/:id', authenticateWebSession(), async (c) => {
  const id = c.req.param('id');
  const entry = await findOwnedLockedPassword(c.env.DB, id, c.get('sub'));

  if (!entry) {
    return c.json({ error: 'Not found' }, 404);
  }

  await markLockedPasswordDeleted(c.env.DB, id, Date.now());

  return c.body(null, 204);
});

lockedPasswords.post('/:id/restore', authenticateWebSession(), async (c) => {
  const id = c.req.param('id');
  const entry = await findOwnedLockedPassword(c.env.DB, id, c.get('sub'));

  if (!entry) {
    return c.json({ error: 'Not found' }, 404);
  }

  await restoreLockedPassword(c.env.DB, id);

  return c.body(null, 204);
});

lockedPasswords.delete('/:id/permanent', authenticateWebSession(), async (c) => {
  const id = c.req.param('id');
  const entry = await findOwnedLockedPassword(c.env.DB, id, c.get('sub'));

  if (!entry) {
    return c.json({ error: 'Not found' }, 404);
  }

  await deleteLockedPasswordById(c.env.DB, id);

  return c.body(null, 204);
});

export default lockedPasswords;
