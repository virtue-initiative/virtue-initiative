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
  findPartnerByInviteTokenHash,
  findPartnerById,
  findPartnerInviteForOwner,
  findPartnerForOwnerAndUser,
  findUserById,
  findUserPublicKeyByEmail,
  listIncomingPartners,
  listOwnedPartners,
  updatePartnerByOwner,
  deletePartnerById,
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

const partnerPermissionsSchema = z.object({
  view_data: z.boolean().optional().default(true),
});

const pubKeyQuerySchema = z.object({
  email: z.email(),
});

const createPartnerSchema = z.object({
  email: z.email(),
  permissions: partnerPermissionsSchema.optional().default({ view_data: true }),
  e2ee_key: z.base64().optional(),
});

const inviteTokenSchema = z.object({
  token: z.string().min(1),
});

const updatePartnerSchema = z
  .object({
    permissions: partnerPermissionsSchema.optional(),
    e2ee_key: z.base64().optional(),
  })
  .refine((data) => Object.keys(data).length > 0, { message: 'No fields to update' });

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
    const { email, permissions } = c.req.valid('json');

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
      user_id: userId,
      partner_email: email,
      invite_token_id: inviteTokenId,
      permissions: JSON.stringify(permissions),
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

partners.post('/partner/invite/validate', validateZ('json', inviteTokenSchema), async (c) => {
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

  const owner = await findUserById(c.env.DB, invite.user_id);
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
  '/partner/invite/accept',
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

    if (invite.user_id === userId) {
      return c.json({ error: 'You cannot accept your own partner invite' }, 409);
    }

    const existing = await findPartnerForOwnerAndUser(c.env.DB, invite.user_id, userId, invite.id);
    if (existing) {
      return c.json({ error: 'Partnership already exists' }, 409);
    }

    await acceptPartner(c.env.DB, {
      id: invite.id,
      partnerUserId: userId,
      partnerEmail: currentUser.email,
      updated_at: Date.now(),
    });
    if (invite.invite_token_id) {
      await consumeEmailToken(c.env.DB, invite.invite_token_id, Date.now());
    }

    const owner = await findUserById(c.env.DB, invite.user_id);
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

  return c.json([
    ...owned.map((partner) => ({
      id: partner.id,
      role: 'owner' as const,
      partner: {
        ...(partner.partner_id ? { id: partner.partner_id } : {}),
        email: partner.partner_email,
        ...(partner.partner_name ? { name: partner.partner_name } : {}),
      },
      status: partner.status,
      permissions: JSON.parse(partner.permissions) as { view_data?: boolean },
      created_at: partner.created_at,
      ...(partner.e2ee_key ? { e2ee_key: encodeBase64(partner.e2ee_key) } : {}),
    })),
    ...incoming.map((partner) => ({
      id: partner.id,
      role: 'invitee' as const,
      partner: {
        id: partner.owner_id,
        email: partner.owner_email,
        ...(partner.owner_name ? { name: partner.owner_name } : {}),
      },
      status: partner.status,
      permissions: JSON.parse(partner.permissions) as { view_data?: boolean },
      created_at: partner.created_at,
      ...(partner.e2ee_key ? { e2ee_key: encodeBase64(partner.e2ee_key) } : {}),
    })),
  ]);
});

partners.patch(
  '/partner/:id',
  authenticate('access'),
  validateZ('json', updatePartnerSchema),
  async (c) => {
    const partnerId = c.req.param('id');
    const partnership = await findPartnerById(c.env.DB, partnerId);

    if (!partnership || partnership.user_id !== c.get('sub')) {
      return c.json({ error: 'Not found' }, 404);
    }

    const { permissions, e2ee_key } = c.req.valid('json');
    const nextPermissions = permissions
      ? { ...(JSON.parse(partnership.permissions) as { view_data?: boolean }), ...permissions }
      : (JSON.parse(partnership.permissions) as { view_data?: boolean });

    await updatePartnerByOwner(c.env.DB, {
      id: partnerId,
      ownerId: c.get('sub'),
      permissions: permissions ? JSON.stringify(nextPermissions) : undefined,
      e2ee_key: e2ee_key ? decodeBase64(e2ee_key) : undefined,
      updated_at: Date.now(),
    });

    return c.json({ id: partnerId, permissions: nextPermissions });
  },
);

partners.delete('/partner/:id', authenticate('access'), async (c) => {
  const partnerId = c.req.param('id');
  const partnership = await findPartnerById(c.env.DB, partnerId);
  const currentUser = await findUserById(c.env.DB, c.get('sub'));

  if (!partnership || !currentUser) {
    return c.json({ error: 'Not found' }, 404);
  }

  const canDeleteAsInvitee =
    partnership.partner_user_id === c.get('sub') ||
    (partnership.partner_user_id === null && partnership.partner_email === currentUser.email);

  if (partnership.user_id !== c.get('sub') && !canDeleteAsInvitee) {
    return c.json({ error: 'Not found' }, 404);
  }

  await deletePartnerById(c.env.DB, partnerId);
  return c.body(null, 204);
});

export default partners;
