import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { generateUploadUrl } from '../lib/r2';
import { createImageSchema } from '../lib/schemas';
import z from 'zod';
import { findDevice, createImage, updateDeviceActivity } from '../lib/db';

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
  const device = await findDevice(c.env.DB, device_id, userId);
  if (!device) return c.json({ error: 'Device not found' }, 404);

  const imageId = uuidv4();

  const r2Key = `user/${userId}/images/${imageId}.webp`;
  const createdAt = new Date().toISOString();

  await createImage(
    c.env.DB,
    imageId,
    userId,
    device_id,
    r2Key,
    sha256,
    content_type,
    size_bytes,
    taken_at,
    createdAt,
  );

  // Update device activity
  await updateDeviceActivity(c.env.DB, device_id, createdAt);

  const uploadUrl = await generateUploadUrl(c.env, r2Key, content_type, size_bytes);

  return c.json(
    {
      image: {
        id: imageId,
        status: 'pending_upload',
        r2_key: r2Key,
        taken_at,
        created_at: createdAt,
      },
      upload_url: uploadUrl,
    },
    201,
  );
});

export default images;
