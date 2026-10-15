import { Hono } from 'hono';
import { z } from 'zod';
import { IOS_BUNDLE_ID, notificationBodySchema, verifyAppStoreJws } from '../lib/app-store';
import { listLiveDeviceIdsByAppTransactionId } from '../lib/db';
import { Env, Variables } from '../types/bindings';
import { logOutDevice } from './device-only';

const appStore = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /app-store/notifications - App Store Server Notifications V2 (API-057).
 *
 * Only RESCIND_CONSENT is acted on: a parent or guardian withdrew consent for
 * a minor, so every device signed in from that download is logged out. Apple
 * retries anything but a 200, so every other notification is acknowledged.
 */
appStore.post('/app-store/notifications', async (c) => {
  const parsed = notificationBodySchema.safeParse(await c.req.json().catch(() => null));
  if (!parsed.success) {
    return c.json({ error: 'Invalid request data', details: z.treeifyError(parsed.error) }, 400);
  }

  const rootSha256 = c.env.APP_STORE_ROOT_SHA256 || undefined;
  const payload = await verifyAppStoreJws(parsed.data.signedPayload, rootSha256);
  if (!payload) {
    return c.json({ error: 'Invalid App Store signature' }, 403);
  }

  if (payload.notificationType !== 'RESCIND_CONSENT') {
    return c.json({ ok: true });
  }

  const appData = payload.appData as
    | { bundleId?: unknown; signedAppTransactionInfo?: unknown }
    | undefined;
  if (appData?.bundleId !== IOS_BUNDLE_ID) {
    return c.json({ ok: true });
  }
  if (typeof appData.signedAppTransactionInfo !== 'string') {
    return c.json({ error: 'Missing app transaction' }, 400);
  }

  const appTransaction = await verifyAppStoreJws(appData.signedAppTransactionInfo, rootSha256);
  if (!appTransaction) {
    return c.json({ error: 'Invalid App Store signature' }, 403);
  }
  if (
    appTransaction.bundleId !== IOS_BUNDLE_ID ||
    typeof appTransaction.appTransactionId !== 'string'
  ) {
    return c.json({ ok: true });
  }

  const deviceIds = await listLiveDeviceIdsByAppTransactionId(
    c.env.DB,
    appTransaction.appTransactionId,
  );
  for (const deviceId of deviceIds) {
    await logOutDevice(c.env, deviceId);
  }

  return c.json({ ok: true, devices: deviceIds.length });
});

export default appStore;
