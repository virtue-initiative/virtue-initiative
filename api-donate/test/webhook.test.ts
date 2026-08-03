import { env, SELF } from 'cloudflare:test';
import { beforeEach, describe, expect, it } from 'vitest';
import { BASE, checkoutCompletedEvent, clearDB, signedWebhook } from './helpers';

beforeEach(clearDB);

async function postWebhook(payload: string, signature: string) {
  return SELF.fetch(`${BASE}/webhook`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'stripe-signature': signature },
    body: payload,
  });
}

describe('POST /webhook', () => {
  it('rejects a request with a bad signature', async () => {
    const res = await postWebhook(JSON.stringify({ id: 'evt_x' }), 't=1,v1=deadbeef');
    expect(res.status).toBe(400);

    const { count } = await env.DB.prepare('SELECT COUNT(*) AS count FROM stripe_events').first<{
      count: number;
    }>();
    expect(count).toBe(0);
  });

  it('records a donation from checkout.session.completed', async () => {
    const { payload, signature } = await signedWebhook(checkoutCompletedEvent());

    const res = await postWebhook(payload, signature);
    expect(res.status).toBe(200);

    const row = await env.DB.prepare('SELECT * FROM donations WHERE id = ?')
      .bind('cs_test_1')
      .first();
    expect(row).toMatchObject({
      email: 'donor@example.com',
      name: 'Donor Example',
      phone: '+15555550123',
      amount_cents: 2500,
      currency: 'usd',
      recurring: 0,
      status: 'completed',
    });
  });

  it('is idempotent when the same event is delivered twice', async () => {
    const event = checkoutCompletedEvent();
    const { payload, signature } = await signedWebhook(event);

    const first = await postWebhook(payload, signature);
    expect(first.status).toBe(200);
    const second = await postWebhook(payload, signature);
    expect(second.status).toBe(200);
    expect(await second.json()).toMatchObject({ duplicate: true });

    const { count } = await env.DB.prepare('SELECT COUNT(*) AS count FROM donations').first<{
      count: number;
    }>();
    expect(count).toBe(1);
  });

  it('marks donations cancelled on customer.subscription.deleted', async () => {
    const created = checkoutCompletedEvent({
      mode: 'subscription',
      subscription: 'sub_test_1',
    });
    const createdSigned = await signedWebhook(created);
    await postWebhook(createdSigned.payload, createdSigned.signature);

    const cancelled = await signedWebhook({
      id: 'evt_cancel_1',
      type: 'customer.subscription.deleted',
      data: { object: { id: 'sub_test_1', object: 'subscription' } },
    });
    const res = await postWebhook(cancelled.payload, cancelled.signature);
    expect(res.status).toBe(200);

    const row = await env.DB.prepare(
      'SELECT status FROM donations WHERE stripe_subscription_id = ?',
    )
      .bind('sub_test_1')
      .first<{ status: string }>();
    expect(row?.status).toBe('cancelled');
  });
});
