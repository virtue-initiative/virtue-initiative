import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { authenticateWebSession } from '../middleware/auth';
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
  listIncomingPartners,
  listOwnedPartners,
} from '../lib/db';
import { renderPartnerAcceptedTemplate, renderPartnerInviteTemplate } from '../lib/email/templates';
import { PARTNER_INVITE_TTL_MS } from '../lib/email-domain';
import {
  createPartnerSchema,
  inviteTokenSchema,
  type CreatePartnerResponse,
} from '../../../shared-web/types';
import { sendEmail } from '../lib/email';
import { serializeWatchers, serializeWatching } from '../lib/serializers';
import { assertTokenPurpose, generateOpaqueToken, hashOpaqueToken } from '../lib/tokens';
import { Env, Variables } from '../types/bindings';

const partners = new Hono<{ Bindings: Env; Variables: Variables }>();

partners.post(
  '/partner',
  authenticateWebSession(),
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
    const inviteToken = generateOpaqueToken('partner_invite');
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
      created_at: now,
    });

    const inviteEmail = renderPartnerInviteTemplate({
      ownerName: currentUser.name,
      ownerEmail: currentUser.email,
      appName: c.env.APP_NAME,
      appUrl: c.env.APP_URL,
      inviteUrl: `${c.env.APP_URL}/invite-accept?partner_token=${encodeURIComponent(inviteToken)}`,
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

    return c.json<CreatePartnerResponse>({ id, status: 'pending' }, 200);
  },
);

partners.post('/partner/validate', validateZ('json', inviteTokenSchema), async (c) => {
  const { token } = c.req.valid('json');

  try {
    assertTokenPurpose(token, 'partner_invite');
  } catch {
    return c.json({ error: 'Invalid or expired invite' }, 400);
  }

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
  authenticateWebSession(),
  validateZ('json', inviteTokenSchema),
  async (c) => {
    const userId = c.get('sub');
    const currentUser = await findUserById(c.env.DB, userId);
    const { token } = c.req.valid('json');

    try {
      assertTokenPurpose(token, 'partner_invite');
    } catch {
      return c.json({ error: 'Invalid or expired invite' }, 400);
    }

    const invite = await findPartnerByInviteTokenHash(c.env.DB, hashOpaqueToken(token));

    if (!currentUser || !invite) {
      return c.json({ error: 'Invalid or expired invite' }, 400);
    }

    if (invite.status === 'accepted' && invite.watcher_user_id === userId) {
      return c.json({ id: invite.id });
    }

    if (invite.status !== 'pending') {
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
      await consumeEmailToken(
        c.env.DB,
        { id: invite.invite_token_id, user_id: null, purpose: 'partner_invite' },
        Date.now(),
      );
    }

    const owner = await findUserById(c.env.DB, invite.watching_user_id);
    if (owner) {
      const acceptedEmail = renderPartnerAcceptedTemplate({
        partnerName: currentUser.name,
        partnerEmail: currentUser.email,
        appName: c.env.APP_NAME,
        appUrl: c.env.APP_URL,
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

partners.get('/partner', authenticateWebSession(), async (c) => {
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
    watching: serializeWatching(incoming),
    watchers: serializeWatchers(owned),
  });
});

partners.delete('/partner/:id', authenticateWebSession(), async (c) => {
  const userId = c.get('sub');
  const partnerId = c.req.param('id');
  const [partnership, currentUser] = await Promise.all([
    findPartnerById(c.env.DB, partnerId),
    findUserById(c.env.DB, userId),
  ]);

  if (!partnership || !currentUser) {
    return c.json({ error: 'Not found' }, 404);
  }

  const canDelete =
    partnership.watching_user_id === userId ||
    partnership.watcher_user_id === userId ||
    (partnership.watcher_user_id === null && partnership.watcher_email === currentUser.email);

  if (!canDelete) {
    return c.json({ error: 'Not found' }, 404);
  }

  await deletePartnerById(c.env.DB, partnerId);
  return c.body(null, 204);
});

export default partners;
