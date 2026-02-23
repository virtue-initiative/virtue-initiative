import z from 'zod';
import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { createPartnerSchema, acceptPartnerSchema, updatePartnerSchema } from '../lib/schemas';
import {
  findUserByEmail,
  findPartnerByUsers,
  createPartner,
  findPartnerInvite,
  acceptPartner,
  listPartners,
  findPartnerByOwner,
  findPartnerByEitherParty,
  updatePartnerPermissions,
  deletePartner,
} from '../lib/db';
import { sendPartnerDeletionEmail } from '../lib/email';

const partners = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /partner - Send partner invite
 */
partners.post('/', authenticate, async (c) => {
  const parsed = createPartnerSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const userId = c.get('userId');
  const { email, permissions } = parsed.data;

  const partnerUser = await findUserByEmail(c.env.DB, email);
  if (!partnerUser) return c.json({ error: 'User not found' }, 404);

  const partnerUserId = partnerUser.id;
  if (partnerUserId === userId) return c.json({ error: 'Cannot add yourself as partner' }, 400);

  const existing = await findPartnerByUsers(c.env.DB, userId, partnerUserId);
  if (existing) return c.json({ error: 'Partnership already exists' }, 409);

  const partnerId = uuidv4();
  const createdAt = new Date().toISOString();

  await createPartner(
    c.env.DB,
    partnerId,
    userId,
    partnerUserId,
    JSON.stringify(permissions),
    createdAt,
  );

  return c.json({ id: partnerId, status: 'pending' }, 201);
});

/**
 * POST /accept-partner - Accept partner invite
 */
partners.post('/accept', authenticate, async (c) => {
  const parsed = acceptPartnerSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const userId = c.get('userId');
  const { id } = parsed.data;

  const partnership = await findPartnerInvite(c.env.DB, id, userId);
  if (!partnership) return c.json({ error: 'Partnership invite not found' }, 404);

  await acceptPartner(c.env.DB, id, new Date().toISOString());

  return c.json({ id });
});

/**
 * GET /partner - List partnerships
 */
partners.get('/', authenticate, async (c) => {
  const userId = c.get('userId');

  const { owned, asPartner } = await listPartners(c.env.DB, userId);

  const map =
    (role: string) =>
    (p: {
      id: string;
      partner_user_id: string;
      partner_email: string;
      status: string;
      permissions: string;
      created_at: string;
    }) => ({
      id: p.id,
      partner_user_id: p.partner_user_id,
      partner_email: p.partner_email,
      status: p.status,
      permissions: JSON.parse(p.permissions),
      role,
      created_at: p.created_at,
    });

  return c.json([...owned.map(map('owner')), ...asPartner.map(map('partner'))]);
});

/**
 * PATCH /partner/:id - Update partner permissions
 */
partners.patch('/:id', authenticate, async (c) => {
  const parsed = updatePartnerSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const userId = c.get('userId');
  const partnerId = c.req.param('id');

  const partnership = await findPartnerByOwner(c.env.DB, partnerId, userId);
  if (!partnership) return c.json({ error: 'Partnership not found' }, 404);

  const current = JSON.parse(partnership.permissions);
  const merged = { ...current, ...parsed.data.permissions };

  await updatePartnerPermissions(
    c.env.DB,
    partnerId,
    JSON.stringify(merged),
    new Date().toISOString(),
  );

  return c.json({ id: partnerId, permissions: merged });
});

/**
 * DELETE /partner/:id - Revoke partner access (either party may delete)
 */
partners.delete('/:id', authenticate, async (c) => {
  const userId = c.get('userId');
  const partnerId = c.req.param('id');

  const partnership = await findPartnerByEitherParty(c.env.DB, partnerId, userId);
  if (!partnership) return c.json({ error: 'Partnership not found' }, 404);

  await deletePartner(c.env.DB, partnerId);

  // Fire-and-forget notification emails
  c.executionCtx.waitUntil(
    sendPartnerDeletionEmail(c.env, partnership.owner_email, partnership.partner_email),
  );

  return c.body(null, 204);
});

export default partners;
