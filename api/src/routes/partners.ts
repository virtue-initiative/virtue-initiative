import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { z } from 'zod';
import { authenticate } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import {
  acceptPartner,
  createPartner,
  deletePartnerById,
  findPartnerById,
  findPartnerInviteForOwner,
  findUserByEmail,
  findUserById,
  findUserPublicKeyByEmail,
  listIncomingPartners,
  listOwnedPartners,
  updatePartnerByOwner,
} from '../lib/db';
import { decodeBase64, encodeBase64 } from '../lib/encoding';
import { Env, Variables } from '../types/bindings';

const partners = new Hono<{ Bindings: Env; Variables: Variables }>();

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

const acceptPartnerSchema = z.object({
  id: z.uuid(),
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
    const { email, permissions, e2ee_key } = c.req.valid('json');

    if (!currentUser) {
      return c.json({ error: 'Not found' }, 404);
    }

    if (currentUser.email === email) {
      return c.json({ error: 'Bad Request', details: { email: ['Cannot invite yourself'] } }, 400);
    }

    const partnerUser = await findUserByEmail(c.env.DB, email);
    const existing = await findPartnerInviteForOwner(c.env.DB, userId, email, partnerUser?.id);

    if (existing) {
      return c.json({ error: 'Partnership already exists' }, 409);
    }

    const id = uuidv4();
    const now = Date.now();

    await createPartner(c.env.DB, {
      id,
      user_id: userId,
      partner_user_id: partnerUser?.id,
      partner_email: email,
      permissions: JSON.stringify(permissions),
      e2ee_key: e2ee_key ? decodeBase64(e2ee_key) : undefined,
      created_at: now,
    });

    return c.json({ id, status: 'pending' }, 201);
  },
);

partners.post(
  '/partner/accept',
  authenticate('access'),
  validateZ('json', acceptPartnerSchema),
  async (c) => {
    const userId = c.get('sub');
    const currentUser = await findUserById(c.env.DB, userId);
    const { id } = c.req.valid('json');
    const invite = await findPartnerById(c.env.DB, id);

    if (!currentUser || !invite || invite.status !== 'pending') {
      return c.json({ error: 'Not found' }, 404);
    }

    if (invite.partner_user_id && invite.partner_user_id !== userId) {
      return c.json({ error: 'Not found' }, 404);
    }

    if (invite.partner_email !== currentUser.email) {
      return c.json({ error: 'Not found' }, 404);
    }

    await acceptPartner(c.env.DB, { id, partnerUserId: userId, updated_at: Date.now() });
    return c.json({ id });
  },
);

partners.get('/partner', authenticate('access'), async (c) => {
  const userId = c.get('sub');
  const [owned, incoming] = await Promise.all([
    listOwnedPartners(c.env.DB, userId),
    listIncomingPartners(c.env.DB, userId),
  ]);

  return c.json([
    ...owned.map((partner) => ({
      id: partner.id,
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

  if (!partnership) {
    return c.json({ error: 'Not found' }, 404);
  }

  if (partnership.user_id !== c.get('sub') && partnership.partner_user_id !== c.get('sub')) {
    return c.json({ error: 'Not found' }, 404);
  }

  await deletePartnerById(c.env.DB, partnerId);
  return c.body(null, 204);
});

export default partners;
