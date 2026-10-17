import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { env, fetchMock, SELF } from 'cloudflare:test';
import { createEmailBounce, listDigestActivity } from '../src/lib/db';
import {
  createSendPacer,
  forEachConcurrently,
  runNotificationSchedule,
} from '../src/lib/scheduler';
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
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

const DAILY_BATCH_ID = '00000000-0000-4000-8000-000000000001';
const WEEKLY_BATCH_ID = '00000000-0000-4000-8000-000000000003';
const OLD_DAILY_BATCH_ID = '00000000-0000-4000-8000-000000000004';
const EMPTY_ACCESS_KEYS = JSON.stringify({ keys: {} });

// Signs up a watcher, verifies their email, has them accept an invite from
// each owner, and saves `settings` (daily digests unless given; new accounts
// default to weekly).
async function addVerifiedWatcher(
  ownerCookies: string | string[],
  email: string,
  settings: Record<string, string> = { email_frequency: 'daily' },
) {
  const { cookie, userId } = await signupAndGetCookie(email, 'pw');
  await markUserEmailVerified(userId);
  for (const ownerCookie of [ownerCookies].flat()) {
    await SELF.fetch(`${BASE}/partner`, {
      method: 'POST',
      headers: authHeaders(ownerCookie),
      body: JSON.stringify({ email }),
    });
    const invite = (await listEmailDeliveries())
      .filter(
        (delivery) => delivery.kind === 'partner_invite' && delivery.recipient_email === email,
      )
      .at(-1);
    const { inviteToken } = JSON.parse(invite!.metadata) as { inviteToken: string };
    await SELF.fetch(`${BASE}/partner/accept`, {
      method: 'POST',
      headers: authHeaders(cookie),
      body: JSON.stringify({ token: inviteToken }),
    });
  }
  await SELF.fetch(`${BASE}/user`, {
    method: 'PATCH',
    headers: authHeaders(cookie),
    body: JSON.stringify({ settings }),
  });
  return userId;
}

// Stores a batch the way POST /d/batch would, with created_at at its end_time
// unless given.
async function insertBatch(input: {
  id: string;
  userId: string;
  deviceId: string;
  start: number;
  end: number;
  high?: number;
  medium?: number;
  createdAt?: number;
}) {
  await env.DB.prepare(
    `INSERT INTO batches (id, user_id, device_id, start_time, end_time, end_hash, access_keys, high_risk_count, medium_risk_count, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
  )
    .bind(
      uuidToBytes(input.id),
      uuidToBytes(input.userId),
      uuidToBytes(input.deviceId),
      input.start,
      input.end,
      'hash',
      EMPTY_ACCESS_KEYS,
      input.high ?? 0,
      input.medium ?? 0,
      input.createdAt ?? input.end,
    )
    .run();
}

async function listDailyDigests() {
  return (await listEmailDeliveries()).filter((delivery) => delivery.kind === 'daily_digest');
}

describe('Notification scheduler', () => {
  it('sends a daily digest at the configured local hour using the prior 24 hours', async () => {
    const now = Date.UTC(2026, 0, 6, 11, 5, 0);
    const previousWindowStart = Date.UTC(2026, 0, 5, 11, 0, 0);
    const withinWindowBatchStart = Date.UTC(2026, 0, 5, 10, 0, 0);
    const withinWindowBatchEnd = Date.UTC(2026, 0, 5, 12, 0, 0);
    const oldBatchStart = Date.UTC(2026, 0, 5, 0, 0, 0);
    const oldBatchEnd = Date.UTC(2026, 0, 5, 2, 0, 0);

    const { cookie: ownerCookie, userId: ownerId } = await signupAndGetCookie(
      'digest-owner@example.com',
      'pw',
    );
    const { cookie: partnerCookie, userId: partnerUserId } = await signupAndGetCookie(
      'digest-partner@example.com',
      'pw',
    );
    await markUserEmailVerified(partnerUserId);
    await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ settings: { email_frequency: 'daily' } }),
    });

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

    const device = await createDeviceForUser(
      'digest-owner@example.com',
      'pw',
      'Digest Device',
      'linux',
    );
    const silentDevice = await createDeviceForUser(
      'digest-owner@example.com',
      'pw',
      'Silent Device',
      'linux',
    );
    await env.DB.prepare('UPDATE devices SET created_at = ? WHERE id IN (?, ?)')
      .bind(previousWindowStart, uuidToBytes(device.id), uuidToBytes(silentDevice.id))
      .run();

    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, start_time, end_time, end_hash, access_keys, high_risk_count, medium_risk_count, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes(DAILY_BATCH_ID),
        uuidToBytes(ownerId),
        uuidToBytes(device.id),
        withinWindowBatchStart,
        withinWindowBatchEnd,
        'hash-1',
        EMPTY_ACCESS_KEYS,
        1,
        0,
        withinWindowBatchEnd,
      )
      .run();

    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, start_time, end_time, end_hash, access_keys, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes(OLD_DAILY_BATCH_ID),
        uuidToBytes(ownerId),
        uuidToBytes(device.id),
        oldBatchStart,
        oldBatchEnd,
        'hash-old',
        EMPTY_ACCESS_KEYS,
        oldBatchEnd,
      )
      .run();

    await runNotificationSchedule(env, now);

    const deliveries = await listEmailDeliveries();
    const digestDelivery = deliveries.find((delivery) => delivery.kind === 'daily_digest');
    expect(digestDelivery?.recipient_email).toBe('digest-partner@example.com');
    expect(digestDelivery?.status).toBe('sent');
    expect(digestDelivery?.text).toContain('Approximate screenshots available: 12');
    expect(digestDelivery?.text).toContain('High risk alerts and screenshots: 1');
    expect(digestDelivery?.text).toContain('Silent Device: no logs in the last 24 hours');
    expect(digestDelivery?.text).toContain(`${env.APP_URL}/settings`);
    // Links to the report for the covered day in the recipient's timezone
    // (06:05 America/New_York on Jan 6 covers Jan 5).
    const reportUrl = `${env.APP_URL}/report/${ownerId}?period=day&date=2026-01-05`;
    expect(digestDelivery?.text).toContain(`Read digest-owner@example.com's report: ${reportUrl}`);
    expect(digestDelivery?.html).toContain(`href="${reportUrl.replace('&', '&amp;')}"`);
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
    await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ settings: { email_frequency: 'daily' } }),
    });

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

    const device = await createDeviceForUser(
      'batch-owner@example.com',
      'pw',
      'Batch Device',
      'linux',
    );
    const silentDevice = await createDeviceForUser(
      'batch-owner@example.com',
      'pw',
      'Silent Device',
      'linux',
    );
    await env.DB.prepare('UPDATE devices SET created_at = ? WHERE id IN (?, ?)')
      .bind(previousWindowStart, uuidToBytes(device.id), uuidToBytes(silentDevice.id))
      .run();

    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, start_time, end_time, end_hash, access_keys, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes(DAILY_BATCH_ID),
        uuidToBytes(ownerId),
        uuidToBytes(device.id),
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
    await SELF.fetch(`${BASE}/user`, {
      method: 'PATCH',
      headers: authHeaders(partnerCookie),
      body: JSON.stringify({ settings: { email_frequency: 'daily' } }),
    });

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

    const ownerOneDevice = await createDeviceForUser(
      'multi-owner-one@example.com',
      'pw',
      'Owner One Device',
      'linux',
    );
    const ownerTwoDevice = await createDeviceForUser(
      'multi-owner-two@example.com',
      'pw',
      'Owner Two Device',
      'linux',
    );

    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, start_time, end_time, end_hash, access_keys, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes('00000000-0000-4000-8000-000000000011'),
        uuidToBytes(ownerOneId),
        uuidToBytes(ownerOneDevice.id),
        previousDayStart,
        previousDayMid,
        'hash-multi-1',
        EMPTY_ACCESS_KEYS,
        previousDayMid,
      )
      .run();

    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, start_time, end_time, end_hash, access_keys, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes('00000000-0000-4000-8000-000000000012'),
        uuidToBytes(ownerTwoId),
        uuidToBytes(ownerTwoDevice.id),
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
    for (const id of [ownerOneId, ownerTwoId]) {
      expect(digestDeliveries[0]?.text).toContain(`${env.APP_URL}/report/${id}?period=day`);
    }
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

    const device = await createDeviceForUser(
      'twice-owner@example.com',
      'pw',
      'Twice Device',
      'linux',
    );
    await env.DB.prepare(
      `INSERT INTO batches (id, user_id, device_id, start_time, end_time, end_hash, access_keys, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        uuidToBytes(WEEKLY_BATCH_ID),
        uuidToBytes(ownerId),
        uuidToBytes(device.id),
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
    // The Monday 06:00 run covers the week ending Sunday, Jan 4.
    expect(digestDelivery?.text).toContain(
      `${env.APP_URL}/report/${ownerId}?period=week&date=2026-01-04`,
    );
  });

  it('keeps sending digests to other watchers when SES rejects one', async () => {
    const now = Date.UTC(2026, 0, 6, 6, 5, 0);
    const { cookie: ownerCookie } = await signupAndGetCookie('ses-owner@example.com', 'pw');
    const watcherEmails = [
      'ses-watcher-a@example.com',
      'ses-watcher-b@example.com',
      'ses-watcher-c@example.com',
    ];
    for (const email of watcherEmails) {
      await addVerifiedWatcher(ownerCookie, email);
    }

    const rejected = 'ses-watcher-b@example.com';
    const sesRecipients: string[] = [];
    fetchMock.activate();
    fetchMock.disableNetConnect();
    fetchMock
      .get('https://email.us-east-1.amazonaws.com')
      .intercept({ path: '/v2/email/outbound-emails', method: 'POST' })
      .reply((opts) => {
        const body = JSON.parse(String(opts.body)) as {
          Destination: { ToAddresses: string[] };
        };
        const recipient = body.Destination.ToAddresses[0];
        sesRecipients.push(recipient);
        if (recipient === rejected) {
          return {
            statusCode: 400,
            data: JSON.stringify({ message: 'Email address is not verified.' }),
            responseOptions: { headers: { 'x-amzn-errortype': 'MessageRejected' } },
          };
        }
        return { statusCode: 200, data: JSON.stringify({ MessageId: `msg-${recipient}` }) };
      })
      .times(watcherEmails.length);
    const errors = vi.spyOn(console, 'error').mockImplementation(() => {});

    try {
      await expect(
        runNotificationSchedule({ ...env, EMAIL_DELIVERY_MODE: 'ses' }, now),
      ).rejects.toThrow('1 digest email(s) failed');
    } finally {
      fetchMock.deactivate();
    }

    expect(sesRecipients.sort()).toEqual(watcherEmails);
    const digests = await listDailyDigests();
    expect(digests.map((digest) => [digest.recipient_email, digest.status])).toEqual([
      [rejected, 'failed'],
    ]);
    expect(errors).toHaveBeenCalledWith('digest email failed', expect.anything());
  });

  it('keeps sending digests to other watchers when one fails to build', async () => {
    const now = Date.UTC(2026, 0, 6, 6, 5, 0);
    const { cookie: ownerCookie } = await signupAndGetCookie('build-owner@example.com', 'pw');
    const brokenId = await addVerifiedWatcher(ownerCookie, 'build-broken@example.com');
    await addVerifiedWatcher(ownerCookie, 'build-ok@example.com');
    // An unknown timezone makes Intl throw while working out the window. It is
    // written directly, since the API refuses to save one.
    await env.DB.prepare('UPDATE users SET settings = ? WHERE id = ?')
      .bind(
        JSON.stringify({ email_frequency: 'daily', timezone: 'Not/AZone' }),
        uuidToBytes(brokenId),
      )
      .run();
    const errors = vi.spyOn(console, 'error').mockImplementation(() => {});

    await expect(runNotificationSchedule(env, now)).rejects.toThrow('1 digest email(s) failed');

    const digests = await listDailyDigests();
    expect(digests.map((digest) => digest.recipient_email)).toEqual(['build-ok@example.com']);
    expect(errors).toHaveBeenCalledWith('digest email failed', expect.anything());
  });

  it('skips watchers whose email is not verified', async () => {
    const now = Date.UTC(2026, 0, 6, 6, 5, 0);
    const { cookie: ownerCookie } = await signupAndGetCookie('unverified-owner@example.com', 'pw');
    const watcherId = await addVerifiedWatcher(ownerCookie, 'unverified-watcher@example.com');
    await env.DB.prepare('UPDATE users SET email_verified = 0 WHERE id = ?')
      .bind(uuidToBytes(watcherId))
      .run();

    await runNotificationSchedule(env, now);

    expect(await listDailyDigests()).toEqual([]);
  });

  it('counts a digest to a permanently bounced address as skipped, not sent', async () => {
    const now = Date.UTC(2026, 0, 6, 6, 5, 0);
    const { cookie: ownerCookie } = await signupAndGetCookie('bounce-owner@example.com', 'pw');
    await addVerifiedWatcher(ownerCookie, 'bounce-watcher@example.com');
    await addVerifiedWatcher(ownerCookie, 'bounce-ok@example.com');
    await createEmailBounce(env.DB, {
      id: crypto.randomUUID(),
      email: 'bounce-watcher@example.com',
      bounced_at: now - 60_000,
      reason: 'permanent',
    });
    // A temporary bounce does not stop a send.
    await createEmailBounce(env.DB, {
      id: crypto.randomUUID(),
      email: 'bounce-ok@example.com',
      bounced_at: now - 60_000,
      reason: 'temporary',
    });
    const info = vi.spyOn(console, 'info').mockImplementation(() => {});

    await runNotificationSchedule(env, now);

    const digests = await listDailyDigests();
    expect(digests.map((digest) => digest.recipient_email)).toEqual(['bounce-ok@example.com']);
    expect(info).toHaveBeenCalledWith('digest run started', { due: 2 });
    expect(info).toHaveBeenCalledWith('digest run finished', {
      due: 2,
      sent: 1,
      skipped: 1,
      failed: 0,
    });
  });

  it('loads digest activity for more watched users than fit in one statement', async () => {
    const { userId: ownerId } = await signupAndGetCookie('many-owner@example.com', 'pw');
    const device = await createDeviceForUser('many-owner@example.com', 'pw', 'Many', 'linux');
    const batchStart = Date.UTC(2026, 0, 5, 12, 0, 0);
    await insertBatch({
      id: DAILY_BATCH_ID,
      userId: ownerId,
      deviceId: device.id,
      start: batchStart,
      end: batchStart + 60_000,
    });

    // The real user is past the first chunk of 90.
    const userIds = Array.from({ length: 150 }, () => crypto.randomUUID());
    userIds[120] = ownerId;
    const rows = await listDigestActivity(
      env.DB,
      userIds,
      { start: Date.UTC(2026, 0, 5, 0, 0, 0), end: Date.UTC(2026, 0, 6, 0, 0, 0) },
      300_000,
    );

    // A one-minute batch still counts as one screenshot. The device was created
    // after this window, so the batch is on none of its days.
    expect(rows).toEqual([
      {
        id: device.id,
        owner: ownerId,
        name: 'Many',
        created_at: expect.any(Number),
        screenshots: 1,
        critical: 0,
        warning: 0,
        first_day: null,
        last_day: null,
      },
    ]);
  });

  it('sums digest activity in SQL and reports the days each device has logs on', async () => {
    const { userId: ownerId } = await signupAndGetCookie('days-owner@example.com', 'pw');
    const device = await createDeviceForUser('days-owner@example.com', 'pw', 'Days', 'linux');
    await env.DB.prepare('UPDATE devices SET created_at = ? WHERE id = ?')
      .bind(Date.UTC(2026, 0, 7, 12, 0, 0), uuidToBytes(device.id))
      .run();
    const batch = (id: string, start: number, end: number, high: number, medium: number) =>
      insertBatch({ id, userId: ownerId, deviceId: device.id, start, end, high, medium });
    // Days count from the device's creation (Jan 7 12:00), so this crosses from
    // day 0 into day 1: 2 hours, 24 screenshots.
    await batch(
      '00000000-0000-4000-8000-000000000021',
      Date.UTC(2026, 0, 8, 11),
      Date.UTC(2026, 0, 8, 13),
      1,
      2,
    );
    // Day 3: 10 minutes, 2 screenshots.
    await batch(
      '00000000-0000-4000-8000-000000000022',
      Date.UTC(2026, 0, 10, 12, 30),
      Date.UTC(2026, 0, 10, 12, 40),
      0,
      1,
    );
    // Before the device existed: counted in the sums, but on no day.
    await batch(
      '00000000-0000-4000-8000-000000000023',
      Date.UTC(2026, 0, 6, 10),
      Date.UTC(2026, 0, 6, 10, 10),
      3,
      0,
    );

    const rows = await listDigestActivity(
      env.DB,
      [ownerId],
      { start: Date.UTC(2026, 0, 5), end: Date.UTC(2026, 0, 12) },
      300_000,
    );

    const groups = rows
      .map((row) => [row.first_day, row.last_day, row.screenshots, row.critical, row.warning])
      .sort((a, b) => (a[0] ?? -1) - (b[0] ?? -1));
    expect(groups).toEqual([
      [null, null, 2, 3, 0],
      [0, 1, 24, 1, 2],
      [3, 3, 2, 0, 1],
    ]);
  });

  it('bounds the digest batch query by created_at', async () => {
    const { userId: ownerId } = await signupAndGetCookie('bound-owner@example.com', 'pw');
    const device = await createDeviceForUser('bound-owner@example.com', 'pw', 'Bound', 'linux');
    const windowStart = Date.UTC(2026, 0, 5, 0, 0, 0);
    const windowEnd = Date.UTC(2026, 0, 6, 0, 0, 0);
    // POST /d/batch keeps end_time <= created_at, so a batch overlapping the
    // window can't have been stored over a day before it. This row breaks that
    // on purpose: it is only left out if the query bounds created_at.
    await insertBatch({
      id: DAILY_BATCH_ID,
      userId: ownerId,
      deviceId: device.id,
      start: windowStart + 60_000,
      end: windowStart + 120_000,
      createdAt: windowStart - 2 * 24 * 60 * 60 * 1000,
    });

    const rows = await listDigestActivity(
      env.DB,
      [ownerId],
      { start: windowStart, end: windowEnd },
      300_000,
    );

    expect(rows.map((row) => row.screenshots)).toEqual([null]);
  });
});

describe('Notification scheduler D1 use', () => {
  // Counts the statements a run prepares and the round trips it makes.
  function countingEnv() {
    const counts = { statements: 0, roundTrips: 0 };
    const db = new Proxy(env.DB, {
      get(target, prop) {
        const value = Reflect.get(target, prop) as unknown;
        if (typeof value !== 'function') return value;
        return (...args: unknown[]) => {
          if (prop === 'prepare') counts.statements++;
          if (prop === 'batch') counts.roundTrips++;
          return (value as (...a: unknown[]) => unknown).apply(target, args);
        };
      },
    });
    return { counts, env: { ...env, DB: db } };
  }

  it('makes the same D1 queries however many digests are due', async () => {
    const now = Date.UTC(2026, 0, 6, 6, 5, 0);
    const ownerCookies = [];
    for (const email of ['load-owner-a@example.com', 'load-owner-b@example.com']) {
      ownerCookies.push((await signupAndGetCookie(email, 'pw')).cookie);
    }
    // Every watcher watches both owners.
    const watcherEmails = [1, 2, 3, 4, 5].map((n) => `load-watcher-${n}@example.com`);
    for (const email of watcherEmails) {
      await addVerifiedWatcher(ownerCookies, email);
    }
    const { counts, env: countedEnv } = countingEnv();

    await runNotificationSchedule(countedEnv, now);

    const digests = await listDailyDigests();
    expect(digests.map((digest) => digest.recipient_email).sort()).toEqual(watcherEmails);
    for (const digest of digests) {
      expect(digest.text).toContain('Monitored accounts: 2');
    }
    // The partnerships (with each recipient's bounce), and one batch of the
    // watched users' activity. Per digest, this was 3 statements before.
    expect(counts).toEqual({ statements: 2, roundTrips: 1 });
  });

  it("counts only each digest's own window when one run covers several", async () => {
    // At 01:05 UTC, 06:00 local has just passed in Asia/Kathmandu (00:15 UTC)
    // and Asia/Karachi (01:00 UTC), so both daily digests are due in this run.
    const now = Date.UTC(2026, 0, 6, 1, 5, 0);
    const { cookie: ownerCookie, userId: ownerId } = await signupAndGetCookie(
      'window-owner@example.com',
      'pw',
    );
    for (const timezone of ['Asia/Kathmandu', 'Asia/Karachi']) {
      const email = `window-${timezone.split('/')[1].toLowerCase()}@example.com`;
      await addVerifiedWatcher(ownerCookie, email, { email_frequency: 'daily', timezone });
    }
    const device = await createDeviceForUser('window-owner@example.com', 'pw', 'W', 'linux');
    const MIN = 60_000;
    // Only in Kathmandu's window (from Jan 5 00:15 UTC): 2 screenshots, 1 critical.
    await insertBatch({
      id: DAILY_BATCH_ID,
      userId: ownerId,
      deviceId: device.id,
      start: Date.UTC(2026, 0, 5, 0, 30),
      end: Date.UTC(2026, 0, 5, 0, 30) + 10 * MIN,
      high: 1,
    });
    // Only in Karachi's window (to Jan 6 01:00 UTC): 4 screenshots, 2 critical.
    await insertBatch({
      id: WEEKLY_BATCH_ID,
      userId: ownerId,
      deviceId: device.id,
      start: Date.UTC(2026, 0, 6, 0, 30),
      end: Date.UTC(2026, 0, 6, 0, 30) + 20 * MIN,
      high: 2,
    });

    await runNotificationSchedule(env, now);

    const digests = await listDailyDigests();
    const textFor = (email: string) =>
      digests.find((digest) => digest.recipient_email === email)?.text;
    expect(textFor('window-kathmandu@example.com')).toContain(
      'Approximate screenshots available: 2',
    );
    expect(textFor('window-karachi@example.com')).toContain('Approximate screenshots available: 4');
    // Tamper counts add up every batch they are given, so these show each
    // digest kept only its own window's batches.
    expect(textFor('window-kathmandu@example.com')).toContain(
      'High risk alerts and screenshots: 1',
    );
    expect(textFor('window-karachi@example.com')).toContain('High risk alerts and screenshots: 2');
  });
});

describe('createSendPacer', () => {
  it('spaces sends at least the interval apart', async () => {
    vi.useFakeTimers();
    const pace = createSendPacer(100);
    const releasedAt: number[] = [];
    const start = Date.now();

    const waits = [0, 1, 2].map(() => pace().then(() => releasedAt.push(Date.now() - start)));
    await vi.advanceTimersByTimeAsync(99);
    expect(releasedAt).toEqual([0]);
    await vi.advanceTimersByTimeAsync(101);
    await Promise.all(waits);

    expect(releasedAt).toEqual([0, 100, 200]);
  });

  it('does not wait when the last send was long enough ago', async () => {
    vi.useFakeTimers();
    const pace = createSendPacer(100);
    await pace();
    await vi.advanceTimersByTimeAsync(500);

    let released = false;
    void pace().then(() => (released = true));
    await vi.advanceTimersByTimeAsync(0);

    expect(released).toBe(true);
  });
});

describe('forEachConcurrently', () => {
  it('runs every item with at most the given number in flight', async () => {
    vi.useFakeTimers();
    let inFlight = 0;
    let maxInFlight = 0;
    const done: number[] = [];

    const run = forEachConcurrently([1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 4, async (item) => {
      inFlight++;
      maxInFlight = Math.max(maxInFlight, inFlight);
      await new Promise((resolve) => setTimeout(resolve, 10 * item));
      inFlight--;
      done.push(item);
    });
    await vi.runAllTimersAsync();
    await run;

    expect(maxInFlight).toBe(4);
    expect(done.sort((a, b) => a - b)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
  });
});
