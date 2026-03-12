import { beforeEach, describe, expect, it } from 'vitest';
import { env, SELF } from 'cloudflare:test';
import { BASE, clearDB, markUserEmailVerified, signupAndGetToken, uuidToBytes } from './helpers';

beforeEach(clearDB);

describe('Email webhooks', () => {
  it('marks users unverified on SNS bounce notifications', async () => {
    const { userId } = await signupAndGetToken('bounce@example.com', 'pw');
    await markUserEmailVerified(userId);

    const res = await SELF.fetch(`${BASE}/email/sns`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        Type: 'Notification',
        Message: JSON.stringify({
          eventType: 'Bounce',
          bounce: {
            bouncedRecipients: [{ emailAddress: 'bounce@example.com' }],
          },
        }),
      }),
    });

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, updated: 1 });

    const user = await env.DB.prepare('SELECT email_verified FROM users WHERE id = ?')
      .bind(uuidToBytes(userId))
      .first<{ email_verified: number }>();
    expect(user?.email_verified).toBe(0);
  });

  it('marks users unverified on SNS complaint notifications', async () => {
    const { userId } = await signupAndGetToken('complaint@example.com', 'pw');
    await markUserEmailVerified(userId);

    const res = await SELF.fetch(`${BASE}/email/sns`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        Type: 'Notification',
        Message: JSON.stringify({
          eventType: 'Complaint',
          complaint: {
            complainedRecipients: [{ emailAddress: 'complaint@example.com' }],
          },
        }),
      }),
    });

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, updated: 1 });

    const user = await env.DB.prepare('SELECT email_verified FROM users WHERE id = ?')
      .bind(uuidToBytes(userId))
      .first<{ email_verified: number }>();
    expect(user?.email_verified).toBe(0);
  });
});
