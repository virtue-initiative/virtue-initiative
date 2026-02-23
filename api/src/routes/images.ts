import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { putObject, getObject } from '../lib/r2';
import { uploadImageSchema } from '../lib/schemas';
import z from 'zod';
import { findDevice, createImage, updateDeviceActivity, findImageById, findAcceptedPartnership } from '../lib/db';

const images = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /image - Upload image binary and create metadata record
 */
images.post('/', authenticate, async (c) => {
  let formData: FormData;
  try {
    formData = await c.req.formData();
  } catch {
    return c.json({ error: 'Expected multipart/form-data' }, 400);
  }

  const file = formData.get('file');
  if (!(file instanceof File)) {
    return c.json({ error: { fieldErrors: { file: ['Required'] } } }, 400);
  }

  const parsed = uploadImageSchema.safeParse({
    device_id: formData.get('device_id'),
    sha256: formData.get('sha256'),
    taken_at: formData.get('taken_at'),
  });
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const userId = c.get('userId');
  const { device_id, sha256, taken_at } = parsed.data;
  const content_type = file.type || 'application/octet-stream';
  const size_bytes = file.size;

  const device = await findDevice(c.env.DB, device_id, userId);
  if (!device) return c.json({ error: 'Device not found' }, 404);

  const imageId = uuidv4();
  const r2Key = `user/${userId}/images/${imageId}.webp`;
  const createdAt = new Date().toISOString();

  await putObject(c.env, r2Key, await file.arrayBuffer(), content_type);

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

  await updateDeviceActivity(c.env.DB, device_id, createdAt);

  return c.json(
    {
      image: {
        id: imageId,
        status: 'uploaded',
        r2_key: r2Key,
        taken_at,
        created_at: createdAt,
      },
    },
    201,
  );
});

/**
 * GET /image/:id - Download an image (owner or partner with view_images permission)
 */
images.get('/:id', authenticate, async (c) => {
  const requesterId = c.get('userId');
  const imageId = c.req.param('id');

  const image = await findImageById(c.env.DB, imageId);
  if (!image) return c.json({ error: 'Not found' }, 404);

  if (image.user_id !== requesterId) {
    const partnership = await findAcceptedPartnership(c.env.DB, image.user_id, requesterId);
    if (!partnership) return c.json({ error: 'Not found' }, 404);
    const perms = JSON.parse(partnership.permissions);
    if (!perms.view_images) return c.json({ error: 'Not found' }, 404);
  }

  const object = await getObject(c.env, image.r2_key);
  if (!object) return c.json({ error: 'Not found' }, 404);

  return new Response(object.body, {
    headers: { 'Content-Type': image.content_type },
  });
});

export default images;
