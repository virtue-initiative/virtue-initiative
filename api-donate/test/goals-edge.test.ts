import { env, SELF } from 'cloudflare:test';
import { beforeEach, describe, expect, it } from 'vitest';
import { BASE, checkoutCompletedEvent, signedWebhook } from './helpers';

beforeEach(async () => {
  await env.DB.prepare("DELETE FROM goals WHERE id != 'lawyer'").run();
  await env.DB.prepare('UPDATE goals SET raised_cents = 0, active = 1, sort_order = 0').run();
});

async function getGoals() {
  const res = await SELF.fetch(`${BASE}/goals`);
  expect(res.status).toBe(200);
  return (await res.json()) as { goals: Array<Record<string, unknown>> };
}

async function addGoal(
  id: string,
  fields: {
    sort_order?: number;
    created_at?: number;
    active?: number;
    description?: string | null;
  },
) {
  await env.DB.prepare(
    `INSERT INTO goals (id, title, description, target_cents, raised_cents, active, sort_order, created_at)
     VALUES (?, ?, ?, 5000, 1200, ?, ?, ?)`,
  )
    .bind(
      id,
      `Goal ${id}`,
      fields.description === undefined ? null : fields.description,
      fields.active ?? 1,
      fields.sort_order ?? 0,
      fields.created_at ?? 1000,
    )
    .run();
}

describe('GET /goals (DONATE-010)', () => {
  it('returns exactly the documented fields for the seeded goal', async () => {
    const { goals } = await getGoals();
    expect(goals).toEqual([
      {
        id: 'lawyer',
        title: 'Legal fees',
        description: expect.any(String),
        targetCents: 100000,
        raisedCents: 0,
      },
    ]);
  });

  it('needs no authentication', async () => {
    const res = await SELF.fetch(`${BASE}/goals`, { headers: { Authorization: 'Bearer junk' } });
    expect(res.status).toBe(200);
  });

  it('returns a null description as null', async () => {
    await addGoal('plain', { description: null });
    const { goals } = await getGoals();
    expect(goals.find((g) => g.id === 'plain')).toEqual({
      id: 'plain',
      title: 'Goal plain',
      description: null,
      targetCents: 5000,
      raisedCents: 1200,
    });
  });

  it('orders by sort_order, then by created_at', async () => {
    await env.DB.prepare(
      "UPDATE goals SET sort_order = 5, created_at = 500 WHERE id = 'lawyer'",
    ).run();
    await addGoal('late-first', { sort_order: 1, created_at: 3000 });
    await addGoal('early-first', { sort_order: 1, created_at: 2000 });
    await addGoal('top', { sort_order: -1, created_at: 9000 });

    const { goals } = await getGoals();
    expect(goals.map((g) => g.id)).toEqual(['top', 'early-first', 'late-first', 'lawyer']);
  });

  it('omits inactive goals among active ones and may return an empty list', async () => {
    await addGoal('hidden', { active: 0 });
    await addGoal('shown', { active: 1, sort_order: 1 });
    expect((await getGoals()).goals.map((g) => g.id)).toEqual(['lawyer', 'shown']);

    await env.DB.prepare('UPDATE goals SET active = 0').run();
    expect(await getGoals()).toEqual({ goals: [] });
  });

  it('reports the manual raised_cents, not the sum of recorded donations', async () => {
    await env.DB.prepare("UPDATE goals SET raised_cents = 700 WHERE id = 'lawyer'").run();
    const { payload, signature } = await signedWebhook(
      checkoutCompletedEvent({ eventId: 'evt_goal_1', sessionId: 'cs_goal_1', amountTotal: 99900 }),
    );
    const hook = await SELF.fetch(`${BASE}/webhook`, {
      method: 'POST',
      headers: { 'stripe-signature': signature },
      body: payload,
    });
    expect(hook.status).toBe(200);

    const { goals } = await getGoals();
    expect(goals[0]).toMatchObject({ id: 'lawyer', raisedCents: 700 });
  });
});
