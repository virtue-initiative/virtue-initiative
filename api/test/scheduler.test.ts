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
  signupAndGetToken,
  uuidToBytes,
} from './helpers';

beforeEach(clearDB);

const DAILY_BATCH_ID = '00000000-0000-4000-8000-000000000001';
const DAILY_RISK_LOG_ID = '00000000-0000-4000-8000-000000000002';
const WEEKLY_BATCH_ID = '00000000-0000-4000-8000-000000000003';
const EMPTY_ACCESS_KEYS = JSON.stringify({ keys: [] });

describe('Notification scheduler', () => {
  it('sends a daily digest and mentions devices with no logs without creating gap alerts', async () => {
    const now = Date.UTC(2026, 0, 6, 8, 0, 0);
    const previousDayStart = Date.UTC(2026, 0, 5, 0, 0, 0);
    const previousDayMid = Date.UTC(2026, 0, 5, 12, 0, 0);

    const { token: ownerToken, userId: ownerId } = await signupAndGetToken(
      'digest-owner@example.com',
      'pw',
    );
    const { token: partnerToken, userId: partnerUserId } = await signupAndGetToken(
      'digest-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
      body: JSON.stringify({
        email: 'digest-partner@example.com',
      }),
    });
    const invite = (await inviteRes.json()) as { id: string };
    const inviteDelivery = (await listEmailDeliveries()).find(
      (delivery) =>
        delivery.kind === 'partner_invite' &&
        delivery.recipient_email === 'digest-partner@example.com',
    );
    const inviteMetadata = JSON.parse(inviteDelivery!.metadata) as { inviteToken: string };

    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const device = await createDeviceForUser(ownerToken, 'Digest Device', 'linux');
    const silentDevice = await createDeviceForUser(ownerToken, 'Silent Device', 'linux');
    await env.DB.prepare('UPDATE devices SET created_at = ? WHERE id IN (?, ?)')
      .bind(previousDayStart, uuidToBytes(device.id), uuidToBytes(silentDevice.id))
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
        previousDayStart,
        previousDayMid,
        'hash-1',
        EMPTY_ACCESS_KEYS,
        previousDayMid,
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
        previousDayMid,
        'system_shutdown',
        JSON.stringify({ title: 'Monitoring interruption detected' }),
        0.7,
        previousDayMid,
      )
      .run();

    await runNotificationSchedule(env, now);

    const deliveries = await listEmailDeliveries();
    const digestDelivery = deliveries.find((delivery) => delivery.kind === 'daily_digest');
    expect(digestDelivery?.recipient_email).toBe('digest-partner@example.com');
    expect(digestDelivery?.status).toBe('sent');
    expect(digestDelivery?.text).toContain('Approximate screenshots available: 13');
    expect(digestDelivery?.text).toContain('Warning tamper alerts: 1');
    expect(digestDelivery?.text).toContain('Silent Device: no logs on 2026-01-05');
    expect(digestDelivery?.text).toContain(`${env.APP_URL}/settings`);
    expect(deliveries.some((delivery) => delivery.kind === 'tamper_alert')).toBe(false);
  });

  it('sends weekly digests on Monday', async () => {
    const now = Date.UTC(2026, 0, 5, 8, 0, 0);
    const previousWeekStart = Date.UTC(2025, 11, 29, 0, 0, 0);
    const sundayMid = Date.UTC(2026, 0, 4, 12, 0, 0);

    const { token: ownerToken, userId: ownerId } = await signupAndGetToken(
      'twice-owner@example.com',
      'pw',
    );
    const { token: partnerToken, userId: partnerUserId } = await signupAndGetToken(
      'twice-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);

    const inviteRes = await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerToken),
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
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ token: inviteMetadata.inviteToken }),
    });

    const invite = (await inviteRes.json()) as { id: string };
    await SELF.fetch(`${BASE}/partner/watching/${invite.id}`, {
      method: 'PATCH',
      headers: authHeaders(partnerToken),
      body: JSON.stringify({ digest_cadence: 'weekly' }),
    });

    const device = await createDeviceForUser(ownerToken, 'Twice Device', 'linux');
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
