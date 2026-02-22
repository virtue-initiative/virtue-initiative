import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { generateUploadUrl } from '../lib/r2';
import { createImageSchema } from '../lib/schemas';
import z from 'zod';

const images = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /image - Create image metadata and return a presigned R2 upload URL
 */
images.post('/', authenticate, async (c) => {
  const parsed = createImageSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const userId = c.get('userId');
  const { device_id, sha256, content_type, size_bytes, taken_at } = parsed.data;

  // Verify device belongs to user
  const device = await c.env.DB.prepare(
    'SELECT id FROM devices WHERE id = ? AND user_id = ?'
  ).bind(device_id, userId).first();
  if (!device) return c.json({ error: 'Device not found' }, 404);

  const imageId = uuidv4();

  if (content_type !== 'image/webp') {
    return c.json({ error: 'Only image/webp content type is allowed' }, 400);
  }

  const r2Key = `user/${userId}/images/${imageId}.webp`;
  const createdAt = new Date().toISOString();

  await c.env.DB.prepare(
    `INSERT INTO images (id, user_id, device_id, r2_key, sha256, content_type, size_bytes, status, taken_at, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, 'pending_upload', ?, ?)`
  ).bind(imageId, userId, device_id, r2Key, sha256, content_type, size_bytes, taken_at, createdAt).run();

  // Update device activity
  await c.env.DB.prepare(
    'UPDATE devices SET last_seen_at = ?, last_upload_at = ? WHERE id = ?'
  ).bind(createdAt, createdAt, device_id).run();

  const uploadUrl = await generateUploadUrl(c.env, r2Key, content_type, size_bytes);

  return c.json({
    image: {
      id: imageId,
      status: 'pending_upload',
      r2_key: r2Key,
      taken_at,
      created_at: createdAt,
    },
    upload_url: uploadUrl,
  }, 201);
});

export default images;
