import z from 'zod';
import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { createPartnerSchema, acceptPartnerSchema, updatePartnerSchema } from '../lib/schemas';

const partners = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /partner - Send partner invite
 */
partners.post('/', authenticate, async (c) => {
  const parsed = createPartnerSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);
  
  const userId = c.get('userId');
  const { email, permissions } = parsed.data;
  
  const partnerUser = await c.env.DB.prepare('SELECT id FROM users WHERE email = ?').bind(email).first();
  if (!partnerUser) return c.json({ error: 'User not found' }, 404);
  
  const partnerUserId = partnerUser.id as string;
  if (partnerUserId === userId) return c.json({ error: 'Cannot add yourself as partner' }, 400);
  
  const existing = await c.env.DB.prepare(
    'SELECT id FROM partners WHERE user_id = ? AND partner_user_id = ?'
  ).bind(userId, partnerUserId).first();
  if (existing) return c.json({ error: 'Partnership already exists' }, 409);
  
  const partnerId = uuidv4();
  const createdAt = new Date().toISOString();
  
  await c.env.DB.prepare(
    `INSERT INTO partners (id, user_id, partner_user_id, status, permissions, created_at, updated_at)
     VALUES (?, ?, ?, 'pending', ?, ?, ?)`
  ).bind(partnerId, userId, partnerUserId, JSON.stringify(permissions), createdAt, createdAt).run();
  
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
  
  const partnership = await c.env.DB.prepare(
    `SELECT id FROM partners WHERE id = ? AND partner_user_id = ? AND status = 'pending'`
  ).bind(id, userId).first();
  if (!partnership) return c.json({ error: 'Partnership invite not found' }, 404);
  
  await c.env.DB.prepare(
    `UPDATE partners SET status = 'accepted', updated_at = ? WHERE id = ?`
  ).bind(new Date().toISOString(), id).run();
  
  return c.json({ id });
});

/**
 * GET /partner - List partnerships
 */
partners.get('/', authenticate, async (c) => {
  const userId = c.get('userId');
  
  const [owned, asPartner] = await Promise.all([
    c.env.DB.prepare(
      `SELECT p.id, u.email as partner_email, p.status, p.permissions, p.created_at
       FROM partners p JOIN users u ON p.partner_user_id = u.id
       WHERE p.user_id = ?`
    ).bind(userId).all(),
    c.env.DB.prepare(
      `SELECT p.id, u.email as partner_email, p.status, p.permissions, p.created_at
       FROM partners p JOIN users u ON p.user_id = u.id
       WHERE p.partner_user_id = ?`
    ).bind(userId).all(),
  ]);
  
  const map = (role: string) => (p: Record<string, unknown>) => ({
    id: p.id,
    partner_email: p.partner_email,
    status: p.status,
    permissions: JSON.parse(p.permissions as string),
    role,
    created_at: p.created_at,
  });
  
  return c.json([
    ...owned.results.map(map('owner')),
    ...asPartner.results.map(map('partner')),
  ]);
});

/**
 * PATCH /partner/:id - Update partner permissions
 */
partners.patch('/:id', authenticate, async (c) => {
  const parsed = updatePartnerSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);
  
  const userId = c.get('userId');
  const partnerId = c.req.param('id');
  
  const partnership = await c.env.DB.prepare(
    'SELECT id, permissions FROM partners WHERE id = ? AND user_id = ?'
  ).bind(partnerId, userId).first<{ id: string; permissions: string }>();
  if (!partnership) return c.json({ error: 'Partnership not found' }, 404);
  
  const current = JSON.parse(partnership.permissions);
  const merged = { ...current, ...parsed.data.permissions };
  
  await c.env.DB.prepare(
    'UPDATE partners SET permissions = ?, updated_at = ? WHERE id = ?'
  ).bind(JSON.stringify(merged), new Date().toISOString(), partnerId).run();
  
  return c.json({ id: partnerId, permissions: merged });
});

/**
 * DELETE /partner/:id - Revoke partner access
 */
partners.delete('/:id', authenticate, async (c) => {
  const userId = c.get('userId');
  const partnerId = c.req.param('id');
  
  const partnership = await c.env.DB.prepare(
    'SELECT id FROM partners WHERE id = ? AND user_id = ?'
  ).bind(partnerId, userId).first();
  if (!partnership) return c.json({ error: 'Partnership not found' }, 404);
  
  await c.env.DB.prepare('DELETE FROM partners WHERE id = ?').bind(partnerId).run();
  return c.body(null, 204);
});

export default partners;
