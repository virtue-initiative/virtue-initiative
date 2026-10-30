import { env, SELF } from 'cloudflare:test';
import { beforeEach, describe, expect, it } from 'vitest';
import { BASE, checkoutCompletedEvent, clearDB, signedWebhook, signedWebhookWith } from './helpers';

beforeEach(clearDB);

function postWebhook(payload: string, signature: string | null) {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (signature !== null) headers['stripe-signature'] = signature;
  return SELF.fetch(`${BASE}/webhook`, { method: 'POST', headers, body: payload });
}

async function send(event: Record<string, unknown>) {
  const { payload, signature } = await signedWebhook(event);
  return postWebhook(payload, signature);
}

type Row = Record<string, unknown>;

async function donation(id: string) {
  return env.DB.prepare('SELECT * FROM donations WHERE id = ?').bind(id).first<Row>();
}

async function count(table: 'donations' | 'stripe_events') {
  const row = await env.DB.prepare(`SELECT COUNT(*) AS count FROM ${table}`).first<{
    count: number;
  }>();
  return row!.count;
}

function sessionCompleted(eventId: string, object: Record<string, unknown>) {
  return {
    id: eventId,
    type: 'checkout.session.completed',
    data: { object: { object: 'checkout.session', ...object } },
  };
}

describe('POST /webhook basics (DONATE-011)', () => {
  it('answers a processed event with 200 { ok: true }', async () => {
    const res = await send(checkoutCompletedEvent());
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true });
  });

  it('answers an event of an unhandled type with 200 { ok: true }', async () => {
    const res = await send({ id: 'evt_unhandled_1', type: 'invoice.paid', data: { object: {} } });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true });
  });

  it('needs no other authentication than the signature', async () => {
    const { payload, signature } = await signedWebhook(checkoutCompletedEvent());
    const res = await SELF.fetch(`${BASE}/webhook`, {
      method: 'POST',
      headers: { 'stripe-signature': signature, Authorization: 'Bearer nonsense' },
      body: payload,
    });
    expect(res.status).toBe(200);
  });
});

describe('POST /webhook signature verification (DONATE-012)', () => {
  it('answers a missing stripe-signature header with 400 and records nothing', async () => {
    const { payload } = await signedWebhook(checkoutCompletedEvent());
    const res = await postWebhook(payload, null);
    expect(res.status).toBe(400);
    expect(await res.json()).toEqual({ error: 'Missing stripe-signature header' });
    expect(await count('stripe_events')).toBe(0);
    expect(await count('donations')).toBe(0);
  });

  it('answers an empty stripe-signature header with 400 and records nothing', async () => {
    const { payload } = await signedWebhook(checkoutCompletedEvent());
    const res = await postWebhook(payload, '');
    expect(res.status).toBe(400);
    expect(await count('stripe_events')).toBe(0);
  });

  const now = () => Math.floor(Date.now() / 1000);

  it.each([
    ['a garbage header', async () => ({ header: 'garbage', mutate: false })],
    ['a header with no v1 signature', async () => ({ header: `t=${now()}`, mutate: false })],
    [
      'a header with a wrong v1 signature',
      async () => ({ header: `t=${now()},v1=deadbeef`, mutate: false }),
    ],
    [
      'a signature made with another secret',
      async () => ({
        header: (await signedWebhookWith(checkoutCompletedEvent(), { secret: 'whsec_other' }))
          .signature,
        mutate: false,
      }),
    ],
    [
      'a timestamp far in the past',
      async () => ({
        header: (await signedWebhookWith(checkoutCompletedEvent(), { timestamp: now() - 3600 }))
          .signature,
        mutate: false,
      }),
    ],
    [
      'a body changed after signing',
      async () => ({
        header: (await signedWebhook(checkoutCompletedEvent())).signature,
        mutate: true,
      }),
    ],
  ])('rejects %s with 400 and changes nothing', async (_name, build) => {
    // A donation that the rejected event would overwrite if it were processed.
    const existing = await send(
      checkoutCompletedEvent({ eventId: 'evt_existing', sessionId: 'cs_test_1', amountTotal: 100 }),
    );
    expect(existing.status).toBe(200);
    const eventsBefore = await count('stripe_events');

    const { header, mutate } = await build();
    const payload = JSON.stringify(
      checkoutCompletedEvent({
        eventId: 'evt_test_1',
        sessionId: 'cs_test_1',
        amountTotal: mutate ? 999999 : 2500,
      }),
    );
    const res = await postWebhook(payload, header);

    expect(res.status).toBe(400);
    expect(await res.json()).toEqual({ error: 'Invalid signature' });
    expect(await count('stripe_events')).toBe(eventsBefore);
    expect((await donation('cs_test_1'))?.amount_cents).toBe(100);
  });

  it('accepts a signature timestamp within Stripe default tolerance', async () => {
    const event = checkoutCompletedEvent();
    const { payload, signature } = await signedWebhookWith(event, {
      timestamp: Math.floor(Date.now() / 1000) - 60,
    });
    const res = await postWebhook(payload, signature);
    expect(res.status).toBe(200);
  });

  it('verifies the raw body, so a re-serialized body does not match', async () => {
    const event = checkoutCompletedEvent();
    const { signature } = await signedWebhook(event);
    const res = await postWebhook(JSON.stringify(event, null, 2), signature);
    expect(res.status).toBe(400);
  });
});

describe('POST /webhook idempotency (DONATE-013)', () => {
  it('records the event id and type before handling it', async () => {
    await send(checkoutCompletedEvent({ eventId: 'evt_rec_1' }));
    await send({
      id: 'evt_rec_2',
      type: 'customer.subscription.deleted',
      data: { object: { id: 'sub_x' } },
    });
    const rows = await env.DB.prepare('SELECT id, type FROM stripe_events ORDER BY id').all();
    expect(rows.results).toEqual([
      { id: 'evt_rec_1', type: 'checkout.session.completed' },
      { id: 'evt_rec_2', type: 'customer.subscription.deleted' },
    ]);
  });

  it('records events of unhandled types too, and treats their redelivery as a duplicate', async () => {
    const event = { id: 'evt_other_1', type: 'invoice.paid', data: { object: {} } };
    expect(await (await send(event)).json()).toEqual({ ok: true });
    expect(await count('stripe_events')).toBe(1);
    expect(await (await send(event)).json()).toEqual({ ok: true, duplicate: true });
    expect(await count('stripe_events')).toBe(1);
  });

  it('answers a duplicate with 200 { ok: true, duplicate: true } and does not handle it again', async () => {
    const first = checkoutCompletedEvent({
      eventId: 'evt_dup_1',
      sessionId: 'cs_dup_1',
      amountTotal: 500,
    });
    expect((await send(first)).status).toBe(200);

    // Change the stored donation, then redeliver the same event id with a different
    // payload. Handling it again would overwrite the change.
    await env.DB.prepare(
      "UPDATE donations SET amount_cents = 1, status = 'cancelled' WHERE id = 'cs_dup_1'",
    ).run();
    const redelivery = checkoutCompletedEvent({
      eventId: 'evt_dup_1',
      sessionId: 'cs_dup_1',
      amountTotal: 99999,
    });
    const res = await send(redelivery);

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, duplicate: true });
    expect(await donation('cs_dup_1')).toMatchObject({ amount_cents: 1, status: 'cancelled' });
  });

  it('does not handle a duplicate subscription deletion again', async () => {
    await send(
      checkoutCompletedEvent({ eventId: 'evt_s1', mode: 'subscription', subscription: 'sub_dup' }),
    );
    const cancel = {
      id: 'evt_cancel_dup',
      type: 'customer.subscription.deleted',
      data: { object: { id: 'sub_dup' } },
    };
    await send(cancel);
    await env.DB.prepare("UPDATE donations SET status = 'completed'").run();
    const res = await send(cancel);
    expect(await res.json()).toEqual({ ok: true, duplicate: true });
    expect((await donation('cs_test_1'))?.status).toBe('completed');
  });

  it('treats different event ids independently', async () => {
    expect(
      (await send(checkoutCompletedEvent({ eventId: 'evt_a', sessionId: 'cs_a' }))).status,
    ).toBe(200);
    const res = await send(checkoutCompletedEvent({ eventId: 'evt_b', sessionId: 'cs_b' }));
    expect(await res.json()).toEqual({ ok: true });
    expect(await count('donations')).toBe(2);
  });
});

describe('checkout.session.completed (DONATE-014)', () => {
  it('stores customer, subscription and recurring for a subscription session', async () => {
    await send(
      checkoutCompletedEvent({
        eventId: 'evt_sub',
        sessionId: 'cs_sub',
        mode: 'subscription',
        subscription: 'sub_123',
        customer: 'cus_123',
        amountTotal: 1000,
      }),
    );
    expect(await donation('cs_sub')).toMatchObject({
      stripe_customer_id: 'cus_123',
      stripe_subscription_id: 'sub_123',
      recurring: 1,
      amount_cents: 1000,
      status: 'completed',
    });
  });

  it('stores recurring 0 and a null subscription for a one-time payment', async () => {
    await send(checkoutCompletedEvent({ eventId: 'evt_once', sessionId: 'cs_once' }));
    expect(await donation('cs_once')).toMatchObject({
      recurring: 0,
      stripe_subscription_id: null,
      stripe_customer_id: 'cus_test_1',
    });
  });

  it.each(['payment', 'setup'])(
    'is recurring only for the subscription mode, not %s',
    async (mode) => {
      await send(
        sessionCompleted(`evt_mode_${mode}`, { id: `cs_mode_${mode}`, mode, amount_total: 100 }),
      );
      expect((await donation(`cs_mode_${mode}`))?.recurring).toBe(0);
    },
  );

  it('reads the ids of expanded customer and subscription objects', async () => {
    await send(
      sessionCompleted('evt_expanded', {
        id: 'cs_expanded',
        mode: 'subscription',
        amount_total: 300,
        customer: { id: 'cus_obj', object: 'customer' },
        subscription: { id: 'sub_obj', object: 'subscription' },
      }),
    );
    expect(await donation('cs_expanded')).toMatchObject({
      stripe_customer_id: 'cus_obj',
      stripe_subscription_id: 'sub_obj',
    });
  });

  it('stores null for every optional field the session omits and defaults amount and currency', async () => {
    const res = await send(sessionCompleted('evt_bare', { id: 'cs_bare', mode: 'payment' }));
    expect(res.status).toBe(200);
    expect(await donation('cs_bare')).toMatchObject({
      stripe_customer_id: null,
      stripe_subscription_id: null,
      email: null,
      name: null,
      phone: null,
      amount_cents: 0,
      currency: 'usd',
      recurring: 0,
      status: 'completed',
    });
  });

  it('stores null for the customer_details fields that are missing or null', async () => {
    await send(
      sessionCompleted('evt_partial', {
        id: 'cs_partial',
        mode: 'payment',
        amount_total: 250,
        customer_details: { email: 'only@example.com', name: null },
      }),
    );
    expect(await donation('cs_partial')).toMatchObject({
      email: 'only@example.com',
      name: null,
      phone: null,
      amount_cents: 250,
    });
  });

  it('keeps the currency the session reports', async () => {
    await send(
      sessionCompleted('evt_eur', {
        id: 'cs_eur',
        mode: 'payment',
        amount_total: 100,
        currency: 'eur',
      }),
    );
    expect((await donation('cs_eur'))?.currency).toBe('eur');
  });

  it('overwrites an existing donation for the same session id instead of duplicating it', async () => {
    await send(
      checkoutCompletedEvent({
        eventId: 'evt_first',
        sessionId: 'cs_over',
        amountTotal: 1000,
        email: 'old@example.com',
        mode: 'subscription',
        subscription: 'sub_old',
      }),
    );
    await env.DB.prepare("UPDATE donations SET status = 'cancelled' WHERE id = 'cs_over'").run();

    await send(
      checkoutCompletedEvent({
        eventId: 'evt_second',
        sessionId: 'cs_over',
        amountTotal: 3000,
        email: 'new@example.com',
        mode: 'payment',
        subscription: null,
      }),
    );

    expect(await count('donations')).toBe(1);
    expect(await donation('cs_over')).toMatchObject({
      amount_cents: 3000,
      email: 'new@example.com',
      recurring: 0,
      stripe_subscription_id: null,
      status: 'completed',
    });
  });
});

describe('customer.subscription.deleted (DONATE-015)', () => {
  const deleted = (eventId: string, subscriptionId: string) => ({
    id: eventId,
    type: 'customer.subscription.deleted',
    data: { object: { id: subscriptionId, object: 'subscription' } },
  });

  it('cancels every donation of the subscription and no others', async () => {
    await send(
      checkoutCompletedEvent({
        eventId: 'e1',
        sessionId: 'cs_1',
        mode: 'subscription',
        subscription: 'sub_a',
      }),
    );
    await send(
      checkoutCompletedEvent({
        eventId: 'e2',
        sessionId: 'cs_2',
        mode: 'subscription',
        subscription: 'sub_a',
      }),
    );
    await send(
      checkoutCompletedEvent({
        eventId: 'e3',
        sessionId: 'cs_3',
        mode: 'subscription',
        subscription: 'sub_b',
      }),
    );
    await send(checkoutCompletedEvent({ eventId: 'e4', sessionId: 'cs_4' }));

    const res = await send(deleted('e_del', 'sub_a'));
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true });

    const statuses = Object.fromEntries(
      (
        await env.DB.prepare('SELECT id, status FROM donations').all<{
          id: string;
          status: string;
        }>()
      ).results.map((r) => [r.id, r.status]),
    );
    expect(statuses).toEqual({
      cs_1: 'cancelled',
      cs_2: 'cancelled',
      cs_3: 'completed',
      cs_4: 'completed',
    });
  });

  it('does nothing and still answers 200 when no donation matches', async () => {
    await send(
      checkoutCompletedEvent({
        eventId: 'e1',
        sessionId: 'cs_1',
        mode: 'subscription',
        subscription: 'sub_a',
      }),
    );
    const res = await send(deleted('e_nomatch', 'sub_unknown'));
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true });
    expect((await donation('cs_1'))?.status).toBe('completed');
    expect(await count('donations')).toBe(1);
  });

  it('does not touch one-time donations that have no subscription', async () => {
    await send(checkoutCompletedEvent({ eventId: 'e1', sessionId: 'cs_1' }));
    await send(deleted('e_del', 'sub_a'));
    expect((await donation('cs_1'))?.status).toBe('completed');
  });
});

describe('other events (DONATE-016)', () => {
  it.each([
    'invoice.paid',
    'payment_intent.succeeded',
    'checkout.session.async_payment_succeeded',
    'customer.subscription.updated',
    'some.future.event',
  ])('acknowledges %s with 200 and changes no donation', async (type) => {
    await send(
      checkoutCompletedEvent({
        eventId: 'e1',
        sessionId: 'cs_1',
        mode: 'subscription',
        subscription: 'sub_a',
      }),
    );
    const before = await donation('cs_1');

    const res = await send({
      id: `evt_${type}`,
      type,
      data: {
        object: { id: 'sub_a', subscription: 'sub_a', mode: 'subscription', amount_total: 1 },
      },
    });

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true });
    expect(await donation('cs_1')).toEqual(before);
    expect(await count('donations')).toBe(1);
  });
});
