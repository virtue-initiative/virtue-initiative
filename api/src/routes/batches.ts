import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import z from 'zod';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { putObject } from '../lib/r2';
import { uploadBatchSchema, listBatchesSchema } from '../lib/schemas';
import {
  findDevice,
  createBatch,
  listBatches,
  findBatchById,
  findAcceptedPartnership,
  getDeviceState,
  upsertDeviceState,
} from '../lib/db';

const batches = new Hono<{ Bindings: Env; Variables: Variables }>();

function isArrayBufferReadable(
  value: unknown,
): value is { arrayBuffer: () => Promise<ArrayBuffer> } {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof Reflect.get(value, 'arrayBuffer') === 'function'
  );
}

/**
 * POST /batch — Upload an encrypted, compressed 1-hour batch blob.
 * Multipart form fields: file, device_id, start_time, end_time, item_count, size_bytes
 *
 * start_chain_hash and end_chain_hash are taken from the stored device state —
 * the client no longer supplies them.
 */
batches.post('/', authenticate, async (c) => {
  let formData: FormData;
  try {
    formData = await c.req.formData();
  } catch {
    return c.json({ error: 'Expected multipart/form-data' }, 400);
  }

  const file = formData.get('file');
  if (!isArrayBufferReadable(file)) {
    return c.json({ error: { fieldErrors: { file: ['Required'] } } }, 400);
  }

  const parsed = uploadBatchSchema.safeParse({
    device_id: formData.get('device_id'),
    start_time: formData.get('start_time'),
    end_time: formData.get('end_time'),
    item_count: formData.get('item_count'),
    size_bytes: formData.get('size_bytes'),
  });
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const userId = c.get('userId');
  const { device_id, start_time, end_time, item_count, size_bytes } = parsed.data;

  const device = await findDevice(c.env.DB, device_id, userId);
  if (!device) return c.json({ error: 'Device not found' }, 404);

  // Derive start/end chain hashes from server-stored device state.
  const deviceState = await getDeviceState(c.env.DB, device_id);
  const endChainHash = deviceState
    ? Buffer.from(deviceState.state).toString('hex')
    : '0'.repeat(64);
  const startChainHash = deviceState?.batch_start_state
    ? Buffer.from(deviceState.batch_start_state).toString('hex')
    : '0'.repeat(64);

  const batchId = uuidv4();
  const r2Key = `user/${userId}/batches/${batchId}.enc`;
  const createdAt = new Date().toISOString();

  await putObject(c.env, r2Key, await file.arrayBuffer(), 'application/octet-stream');

  await createBatch(
    c.env.DB,
    batchId,
    userId,
    device_id,
    r2Key,
    start_time,
    end_time,
    startChainHash,
    endChainHash,
    item_count,
    size_bytes,
    createdAt,
  );

  // Reset device state to random bytes at batch boundary.
  // Store the same value as both the new current state and the next batch's start state.
  const newStateBytes = crypto.getRandomValues(new Uint8Array(32));
  await upsertDeviceState(
    c.env.DB,
    device_id,
    userId,
    newStateBytes.buffer,
    createdAt,
    newStateBytes.buffer,
  );
  const newStateHex = Buffer.from(newStateBytes).toString('hex');

  return c.json(
    {
      batch: {
        id: batchId,
        r2_key: r2Key,
        start_time,
        end_time,
        created_at: createdAt,
      },
      new_state_hex: newStateHex,
    },
    201,
  );
});

/**
 * GET /batch — List batches for the authenticated user (or a partner with view_data).
 */
batches.get('/', authenticate, async (c) => {
  const parsed = listBatchesSchema.safeParse(c.req.query());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const requesterId = c.get('userId');
  const { device_id, user, cursor, limit } = parsed.data;
  const targetId = user ?? requesterId;

  if (targetId !== requesterId) {
    const partnership = await findAcceptedPartnership(c.env.DB, targetId, requesterId);
    if (!partnership) return c.json({ error: 'Forbidden' }, 403);
    const perms = JSON.parse(partnership.permissions);
    if (!perms.view_data) return c.json({ error: 'Forbidden' }, 403);
  }

  const { items, hasMore } = await listBatches(c.env.DB, targetId, { device_id, cursor }, limit);

  return c.json({
    items,
    ...(hasMore && { next_cursor: items[items.length - 1].created_at }),
  });
});

export default batches;
