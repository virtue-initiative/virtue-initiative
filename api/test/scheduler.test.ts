import { beforeEach, describe, expect, it } from 'vitest';
import { env, SELF } from 'cloudflare:test';
import { runNotificationSchedule } from '../src/lib/scheduler';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetCookie,
  uuidToBytes,
} from './helpers';

beforeEach(clearDB);

const DAILY_BATCH_ID = '00000000-0000-4000-8000-000000000001';
const DAILY_RISK_LOG_ID = '00000000-0000-4000-8000-000000000002';
const WEEKLY_BATCH_ID = '00000000-0000-4000-8000-000000000003';
const OLD_DAILY_BATCH_ID = '00000000-0000-4000-8000-000000000004';
const EMPTY_ACCESS_KEYS = JSON.stringify({ keys: [] });

describe('Notification scheduler', () => {
  it('sends a daily digest at the configured local hour using the prior 24 hours', async () => {
    const now = Date.UTC(2026, 0, 6, 11, 5, 0);
    const previousWindowStart = Date.UTC(2026, 0, 5, 11, 0, 0);
    const withinWindowBatchStart = Date.UTC(2026, 0, 5, 10, 0, 0);
    const withinWindowBatchEnd = Date.UTC(2026, 0, 5, 12, 0, 0);
    const oldBatchStart = Date.UTC(2026, 0, 5, 0, 0, 0);
    const oldBatchEnd = Date.UTC(2026, 0, 5, 2, 0, 0);
    const riskLogTime = Date.UTC(2026, 0, 5, 15, 0, 0);

    const { cookie: ownerCookie, userId: ownerId } = await signupAndGetCookie(
      'digest-owner@example.com',
      'pw',
    );
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'digest-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({
        email: 'digest-partner@example.com',
      }),
    });
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'digest-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });
    await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({
        settings: { timezone: 'America/New_York' },
      }),
    });

    const device = await createDeviceForUser(ownerCookie, 'Digest Device', 'linux');
    const silentDevice = await createDeviceForUser(ownerCookie, 'Silent Device', 'linux');
    await env.DB.prepare('UPDATE devices SET created_at = ? WHERE id IN (?, ?)')
      .bind(previousWindowStart, uuidToBytes(device.id), uuidToBytes(silentDevice.id))
      .run();

    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, url, start_time, end_time, end_hash, access_keys, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes(DAILY_BATCH_ID),
        uuidToBytes(ownerId),
        uuidToBytes(device.id),
        'https://example.com/batch-1.enc',
        withinWindowBatchStart,
        withinWindowBatchEnd,
        'hash-1',
        EMPTY_ACCESS_KEYS,
        withinWindowBatchEnd,
      )
      .run();

    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, url, start_time, end_time, end_hash, access_keys, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes(OLD_DAILY_BATCH_ID),
        uuidToBytes(ownerId),
        uuidToBytes(device.id),
        'https://example.com/old-batch.enc',
        oldBatchStart,
        oldBatchEnd,
        'hash-old',
        EMPTY_ACCESS_KEYS,
        oldBatchEnd,
      )
      .run();

    await env.DB.prepare(
      `INSERT INTO device_logs (id, user_id, device_id, ts, type, data, risk, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes(DAILY_RISK_LOG_ID),
        uuidToBytes(ownerId),
        uuidToBytes(device.id),
        riskLogTime,
        'system_shutdown',
        JSON.stringify({ title: 'Monitoring interruption detected' }),
        0.7,
        riskLogTime,
      )
      .run();

    await runNotificationSchedule(env, now);

    const deliveries = await listEmailDeliveries();
    const digestDelivery = deliveries.find((delivery) => delivery.kind === 'daily_digest');
    expect(digestDelivery?.recipient_email).toBe('digest-partner@example.com');
    expect(digestDelivery?.status).toBe('sent');
    expect(digestDelivery?.text).toContain('Approximate screenshots available: 12');
    expect(digestDelivery?.text).toContain('Critical tamper alerts: 1');
    expect(digestDelivery?.text).toContain('Silent Device: no logs in the last 24 hours');
    expect(digestDelivery?.text).toContain(`${env.APP_URL}/settings`);
    expect(deliveries.some((delivery) => delivery.kind === 'tamper_alert')).toBe(false);
  });

  it('treats batch uploads as activity when deciding whether logs are missing', async () => {
    const now = Date.UTC(2026, 0, 6, 6, 5, 0);
    const previousWindowStart = Date.UTC(2026, 0, 5, 6, 0, 0);
    const batchStart = Date.UTC(2026, 0, 5, 12, 0, 0);
    const batchEnd = Date.UTC(2026, 0, 5, 13, 0, 0);

    const { cookie: ownerCookie, userId: ownerId } = await signupAndGetCookie(
      'batch-owner@example.com',
      'pw',
    );
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'batch-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({
        email: 'batch-partner@example.com',
      }),
    });
    await inviteRes.json();
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'batch-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const device = await createDeviceForUser(ownerCookie, 'Batch Device', 'linux');
    const silentDevice = await createDeviceForUser(ownerCookie, 'Silent Device', 'linux');
    await env.DB.prepare('UPDATE devices SET created_at = ? WHERE id IN (?, ?)')
      .bind(previousWindowStart, uuidToBytes(device.id), uuidToBytes(silentDevice.id))
      .run();

    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, url, start_time, end_time, end_hash, access_keys, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes(DAILY_BATCH_ID),
        uuidToBytes(ownerId),
        uuidToBytes(device.id),
        'https://example.com/batch-only.enc',
        batchStart,
        batchEnd,
        'hash-batch-only',
        EMPTY_ACCESS_KEYS,
        batchEnd,
      )
      .run();

    await runNotificationSchedule(env, now);

    const digestDelivery = (await listEmailDeliveries()).find(
      (delivery) => delivery.kind === 'daily_digest',
    );
    expect(digestDelivery?.text).toContain('Approximate screenshots available: 12');
    expect(digestDelivery?.text).not.toContain('Batch Device: no logs in the last 24 hours');
    expect(digestDelivery?.text).toContain('Silent Device: no logs in the last 24 hours');
  });

  it('sends one daily digest with separate summaries when a partner monitors multiple people', async () => {
    const now = Date.UTC(2026, 0, 6, 6, 5, 0);
    const previousDayStart = Date.UTC(2026, 0, 5, 0, 0, 0);
    const previousDayMid = Date.UTC(2026, 0, 5, 12, 0, 0);

    const { cookie: ownerOneCookie, userId: ownerOneId } = await signupAndGetCookie(
      'multi-owner-one@example.com',
      'pw',
      'Owner One',
    );
    const { cookie: ownerTwoCookie, userId: ownerTwoId } = await signupAndGetCookie(
      'multi-owner-two@example.com',
      'pw',
      'Owner Two',
    );
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'multi-partner@example.com',
      'pw',
      'Partner',
    );
    await markUserEmailVerified(partnerUserId);

    for (const ownerCookie of [ownerOneCookie, ownerTwoCookie]) {
      const inviteRes = await SELF.fetch(`${BASE}/partner`, {
        method: 'POST',
        headers: authHeaders(ownerCookie),
        body: JSON.stringify({
          email: 'multi-partner@example.com',
        }),
      });
      await inviteRes.json();
    }

    const inviteDeliveries = (await listEmailDeliveries()).filter(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'multi-partner@example.com',
    );

    for (const delivery of inviteDeliveries) {
      const inviteMetadata = JSON.parse(delivery.metadata) as { inviteToken: string };
      await SELF.fetch(`${BASE}/partner/accept`, {
        method: 'POST',
        headers: authHeaders(partnerCookie),
        body: JSON.stringify({ token: inviteMetadata.inviteToken }),
      });
    }

    const ownerOneDevice = await createDeviceForUser(ownerOneCookie, 'Owner One Device', 'linux');
    const ownerTwoDevice = await createDeviceForUser(ownerTwoCookie, 'Owner Two Device', 'linux');

    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, url, start_time, end_time, end_hash, access_keys, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes('00000000-0000-4000-8000-000000000011'),
        uuidToBytes(ownerOneId),
        uuidToBytes(ownerOneDevice.id),
        'https://example.com/multi-batch-1.enc',
        previousDayStart,
        previousDayMid,
        'hash-multi-1',
        EMPTY_ACCESS_KEYS,
        previousDayMid,
      )
      .run();

    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, url, start_time, end_time, end_hash, access_keys, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes('00000000-0000-4000-8000-000000000012'),
        uuidToBytes(ownerTwoId),
        uuidToBytes(ownerTwoDevice.id),
        'https://example.com/multi-batch-2.enc',
        previousDayStart,
        previousDayMid,
        'hash-multi-2',
        EMPTY_ACCESS_KEYS,
        previousDayMid,
      )
      .run();

    await runNotificationSchedule(env, now);

    const deliveries = await listEmailDeliveries();
    const digestDeliveries = deliveries.filter((delivery) => delivery.kind === 'daily_digest');
    expect(digestDeliveries).toHaveLength(1);
    expect(digestDeliveries[0]?.recipient_email).toBe('multi-partner@example.com');
    expect(digestDeliveries[0]?.text).toContain('Monitored accounts: 2');
    expect(digestDeliveries[0]?.text).toContain('Owner One');
    expect(digestDeliveries[0]?.text).toContain('Owner Two');
  });

  it('sends weekly digests on Monday', async () => {
    const now = Date.UTC(2026, 0, 5, 6, 5, 0);
    const previousWeekStart = Date.UTC(2025, 11, 29, 0, 0, 0);
    const sundayMid = Date.UTC(2026, 0, 4, 12, 0, 0);

    const { cookie: ownerCookie, userId: ownerId } = await signupAndGetCookie(
      'twice-owner@example.com',
      'pw',
    );
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'twice-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({
        email: 'twice-partner@example.com',
      }),
    });
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'twice-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    await inviteRes.json();
    await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ settings: { email_frequency: 'weekly' } }),
    });

    const device = await createDeviceForUser(ownerCookie, 'Twice Device', 'linux');
    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, url, start_time, end_time, end_hash, access_keys, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes(WEEKLY_BATCH_ID),
        uuidToBytes(ownerId),
        uuidToBytes(device.id),
        'https://example.com/batch-twice.enc',
        previousWeekStart,
        sundayMid,
        'hash-twice',
        EMPTY_ACCESS_KEYS,
        sundayMid,
      )
      .run();

    await runNotificationSchedule(env, now);

    const deliveries = await listEmailDeliveries();
    const digestDelivery = deliveries.find((delivery) => delivery.kind === 'weekly_digest');
    expect(digestDelivery?.recipient_email).toBe('twice-partner@example.com');
  });
});
