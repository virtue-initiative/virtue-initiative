import { env, SELF } from 'cloudflare:test';
import { beforeEach, describe, expect, it } from 'vitest';
import { BASE } from './helpers';

beforeEach(async () => {
  await env.DB.prepare('UPDATE goals SET raised_cents = 0, active = 1').run();
});

describe('GET /goals (DONATE-010)', () => {
  it('returns the seeded goal with zero raised', async () => {
    const res = await SELF.fetch(`${BASE}/goals`);
    expect(res.status).toBe(200);
    const body = (await res.json()) as { goals: Array<Record<string, unknown>> };
    expect(body.goals[0]).toMatchObject({ id: 'lawyer', targetCents: 100000, raisedCents: 0 });
  });

  it('reports the manually entered raised amount and hides inactive goals', async () => {
    await env.DB.prepare("UPDATE goals SET raised_cents = 3500 WHERE id = 'lawyer'").run();
    let body = (await (await SELF.fetch(`${BASE}/goals`)).json()) as {
      goals: Array<{ raisedCents: number }>;
    };
    expect(body.goals[0].raisedCents).toBe(3500);

    await env.DB.prepare("UPDATE goals SET active = 0 WHERE id = 'lawyer'").run();
    body = (await (await SELF.fetch(`${BASE}/goals`)).json()) as typeof body;
    expect(body.goals).toHaveLength(0);
  });
});
