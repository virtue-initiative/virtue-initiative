import { Context, Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { z } from 'zod';
import { authenticate } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import {
  acceptPartner,
  consumeEmailToken,
  createPartner,
  createEmailToken,
  deletePartnerById,
  findPartnerByInviteTokenHash,
  findPartnerById,
  findPartnerInviteForOwner,
  findPartnerForOwnerAndUser,
  findUserById,
  findUserPublicKeyByEmail,
  listIncomingPartners,
  listOwnedPartners,
  updatePartnerByOwner,
  updatePartnerNotificationPreference,
  upsertPartnerPreference,
} from '../lib/db';
import { renderPartnerAcceptedTemplate, renderPartnerInviteTemplate } from '../lib/email/templates';
import { PARTNER_INVITE_TTL_MS } from '../lib/email-domain';
import { sendEmail } from '../lib/email';
import { DEFAULT_EMAIL_FREQUENCY, DEFAULT_IMMEDIATE_TAMPER_SEVERITY } from '../lib/email-domain';
import { decodeBase64, encodeBase64 } from '../lib/encoding';
import { generateOpaqueToken, hashOpaqueToken } from '../lib/tokens';
import { Env, Variables } from '../types/bindings';

const partners = new Hono<{ Bindings: Env; Variables: Variables }>();
const LOCAL_WEB_URL = 'http://localhost:5173';

function getAppUrl(c: Context<{ Bindings: Env; Variables: Variables }>) {
  const requestUrl = new URL(c.req.url);
  if (requestUrl.hostname === 'localhost' || requestUrl.hostname === '127.0.0.1') {
    return LOCAL_WEB_URL;
  }

  return c.env.APP_URL;
}

const pubKeyQuerySchema = z.object({
  email: z.email(),
});

const createPartnerSchema = z.object({
  email: z.email(),
  e2ee_key: z.base64().optional(),
});

const inviteTokenSchema = z.object({
  token: z.string().min(1),
});

const updateWatcherSchema = z
  .object({
    e2ee_key: z.base64().optional(),
  })
  .refine((data) => Object.keys(data).length > 0, { message: 'No fields to update' });

const publicNotificationCadences = ['none', 'alerts-only', 'daily', 'weekly'] as const;

const updateWatchingSchema = z
  .object({
    digest_cadence: z.enum(publicNotificationCadences).optional(),
    immediate_tamper_severity: z.enum(['warning', 'critical']).optional(),
  })
  .refine((data) => Object.keys(data).length > 0, { message: 'No fields to update' });

function toPublicNotificationCadence(
  emailFrequency: string | null | undefined,
) {
  if (!emailFrequency || !publicNotificationCadences.includes(emailFrequency as never)) {
    return 'daily' as const;
  }

  return emailFrequency as (typeof publicNotificationCadences)[number];
}

partners.get('/pubkey', validateZ('query', pubKeyQuerySchema), async (c) => {
  const { email } = c.req.valid('query');
  const user = await findUserPublicKeyByEmail(c.env.DB, email);

  if (!user?.pub_key) {
    return c.json({ error: 'Not found' }, 404);
  }

  return c.json({ pubkey: encodeBase64(user.pub_key) });
});

partners.post(
  '/partner',
  authenticate('access'),
  validateZ('json', createPartnerSchema),
  async (c) => {
    const userId = c.get('sub');
    const currentUser = await findUserById(c.env.DB, userId);
    const { email } = c.req.valid('json');

    if (!currentUser) {
      return c.json({ error: 'Not found' }, 404);
    }

    if (currentUser.email === email) {
      return c.json({ error: 'Bad Request', details: { email: ['Cannot invite yourself'] } }, 400);
    }

    const existing = await findPartnerInviteForOwner(c.env.DB, userId, email);

    if (existing) {
      return c.json({ error: 'Partnership already exists' }, 409);
    }

    const id = uuidv4();
    const inviteTokenId = uuidv4();
    const now = Date.now();
    const inviteToken = generateOpaqueToken();
    const inviteTokenHash = hashOpaqueToken(inviteToken);

    await createEmailToken(c.env.DB, {
      id: inviteTokenId,
      user_id: null,
      email,
      purpose: 'partner_invite',
      token_hash: inviteTokenHash,
      expires_at: now + PARTNER_INVITE_TTL_MS,
      created_at: now,
    });

    await createPartner(c.env.DB, {
      id,
      watching_user_id: userId,
      watcher_email: email,
      invite_token_id: inviteTokenId,
      e2ee_key: undefined,
      created_at: now,
    });
    await upsertPartnerPreference(c.env.DB, {
      partnership_id: id,
      email_frequency: DEFAULT_EMAIL_FREQUENCY,
      immediate_tamper_severity: DEFAULT_IMMEDIATE_TAMPER_SEVERITY,
      updated_at: now,
    });

    const inviteEmail = renderPartnerInviteTemplate({
      ownerName: currentUser.name,
      ownerEmail: currentUser.email,
      appName: c.env.APP_NAME,
      appUrl: getAppUrl(c),
      inviteUrl: `${getAppUrl(c)}/?partner_invite_token=${encodeURIComponent(inviteToken)}`,
    });
    await sendEmail({
      env: c.env,
      db: c.env.DB,
      kind: 'partner_invite',
      recipient: email,
      subject: inviteEmail.subject,
      text: inviteEmail.text,
      html: inviteEmail.html,
      related_user_id: userId,
      related_partnership_id: id,
      metadata: { partnerEmail: email, inviteToken },
    });

    return c.json({ id, status: 'pending' }, 201);
  },
);

partners.post('/partner/validate', validateZ('json', inviteTokenSchema), async (c) => {
  const { token } = c.req.valid('json');
  const invite = await findPartnerByInviteTokenHash(c.env.DB, hashOpaqueToken(token));

  if (
    !invite ||
    invite.status !== 'pending' ||
    invite.invite_consumed_at ||
    !invite.invite_expires_at ||
    invite.invite_expires_at < Date.now()
  ) {
    return c.json({ error: 'Invalid or expired invite' }, 400);
  }

  const owner = await findUserById(c.env.DB, invite.watching_user_id);
  if (!owner) {
    return c.json({ error: 'Invalid or expired invite' }, 400);
  }

  return c.json({
    ok: true,
    partnership_id: invite.id,
    owner: {
      id: owner.id,
      email: owner.email,
      ...(owner.name ? { name: owner.name } : {}),
    },
  });
});

partners.post(
  '/partner/accept',
  authenticate('access'),
  validateZ('json', inviteTokenSchema),
  async (c) => {
    const userId = c.get('sub');
    const currentUser = await findUserById(c.env.DB, userId);
    const { token } = c.req.valid('json');
    const invite = await findPartnerByInviteTokenHash(c.env.DB, hashOpaqueToken(token));

    if (!currentUser || !invite || invite.status !== 'pending') {
      return c.json({ error: 'Invalid or expired invite' }, 400);
    }

    if (
      invite.invite_consumed_at ||
      !invite.invite_expires_at ||
      invite.invite_expires_at < Date.now()
    ) {
      return c.json({ error: 'Invalid or expired invite' }, 400);
    }

    if (invite.watching_user_id === userId) {
      return c.json({ error: 'You cannot accept your own partner invite' }, 409);
    }

    const existing = await findPartnerForOwnerAndUser(
      c.env.DB,
      invite.watching_user_id,
      userId,
      invite.id,
    );
    if (existing) {
      return c.json({ error: 'Partnership already exists' }, 409);
    }

    await acceptPartner(c.env.DB, {
      id: invite.id,
      watcherUserId: userId,
      watcherEmail: currentUser.email,
      updated_at: Date.now(),
    });
    if (invite.invite_token_id) {
      await consumeEmailToken(c.env.DB, invite.invite_token_id, Date.now());
    }

    const owner = await findUserById(c.env.DB, invite.watching_user_id);
    if (owner) {
      const acceptedEmail = renderPartnerAcceptedTemplate({
        partnerName: currentUser.name,
        partnerEmail: currentUser.email,
        appName: c.env.APP_NAME,
        appUrl: getAppUrl(c),
      });
      await sendEmail({
        env: c.env,
        db: c.env.DB,
        kind: 'partner_accepted',
        recipient: owner.email,
        subject: acceptedEmail.subject,
        text: acceptedEmail.text,
        html: acceptedEmail.html,
        related_user_id: owner.id,
        related_partnership_id: invite.id,
        metadata: { acceptedBy: currentUser.email },
      });
    }

    return c.json({ id: invite.id });
  },
);

partners.get('/partner', authenticate('access'), async (c) => {
  const userId = c.get('sub');
  const currentUser = await findUserById(c.env.DB, userId);

  if (!currentUser) {
    return c.json({ error: 'Not found' }, 404);
  }

  const [owned, incoming] = await Promise.all([
    listOwnedPartners(c.env.DB, userId),
    listIncomingPartners(c.env.DB, userId),
  ]);

  return c.json({
    watching: incoming.map((partner) => ({
      id: partner.id,
      user: {
        id: partner.watching_user_id,
        email: partner.watching_user_email,
        ...(partner.watching_user_name ? { name: partner.watching_user_name } : {}),
      },
      status: partner.status,
      digest_cadence: toPublicNotificationCadence(partner.email_frequency),
      immediate_tamper_severity:
        partner.immediate_tamper_severity === 'warning' ? 'warning' : 'critical',
      created_at: partner.created_at,
      ...(partner.e2ee_key ? { e2ee_key: encodeBase64(partner.e2ee_key) } : {}),
    })),
    watchers: owned.map((partner) => ({
      id: partner.id,
      user: {
        ...(partner.watcher_id ? { id: partner.watcher_id } : {}),
        email: partner.watcher_email,
        ...(partner.watcher_name ? { name: partner.watcher_name } : {}),
      },
      status: partner.status,
      created_at: partner.created_at,
      ...(partner.e2ee_key ? { e2ee_key: encodeBase64(partner.e2ee_key) } : {}),
    })),
  });
});

partners.patch(
  '/partner/watcher/:id',
  authenticate('access'),
  validateZ('json', updateWatcherSchema),
  async (c) => {
    const partnerId = c.req.param('id');
    const partnership = await findPartnerById(c.env.DB, partnerId);

    if (!partnership || partnership.watching_user_id !== c.get('sub')) {
      return c.json({ error: 'Not found' }, 404);
    }

    const { e2ee_key } = c.req.valid('json');

    await updatePartnerByOwner(c.env.DB, {
      id: partnerId,
      ownerId: c.get('sub'),
      e2ee_key: e2ee_key ? decodeBase64(e2ee_key) : undefined,
      updated_at: Date.now(),
    });

    return c.body(null, 204);
  },
);

partners.patch(
  '/partner/watching/:id',
  authenticate('access'),
  validateZ('json', updateWatchingSchema),
  async (c) => {
    const { digest_cadence, immediate_tamper_severity } = c.req.valid('json');

    const result = await updatePartnerNotificationPreference(c.env.DB, {
      partnership_id: c.req.param('id'),
      watcher_user_id: c.get('sub'),
      updated_at: Date.now(),
      ...(digest_cadence ? { email_frequency: digest_cadence } : {}),
      ...(immediate_tamper_severity ? { immediate_tamper_severity } : {}),
    });

    if (!result) {
      return c.json({ error: 'Not found' }, 404);
    }

    return c.body(null, 204);
  },
);

partners.delete('/partner/watcher/:id', authenticate('access'), async (c) => {
  const partnerId = c.req.param('id');
  const partnership = await findPartnerById(c.env.DB, partnerId);

  if (!partnership || partnership.watching_user_id !== c.get('sub')) {
    return c.json({ error: 'Not found' }, 404);
  }

  await deletePartnerById(c.env.DB, partnerId);
  return c.body(null, 204);
});

partners.delete('/partner/watching/:id', authenticate('access'), async (c) => {
  const partnerId = c.req.param('id');
  const partnership = await findPartnerById(c.env.DB, partnerId);
  const currentUser = await findUserById(c.env.DB, c.get('sub'));

  if (!partnership || !currentUser) {
    return c.json({ error: 'Not found' }, 404);
  }

  const canDelete =
    partnership.watcher_user_id === c.get('sub') ||
    (partnership.watcher_user_id === null && partnership.watcher_email === currentUser.email);

  if (!canDelete) {
    return c.json({ error: 'Not found' }, 404);
  }

  await deletePartnerById(c.env.DB, partnerId);
  return c.body(null, 204);
});

export default partners;
