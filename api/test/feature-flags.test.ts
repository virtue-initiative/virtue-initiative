import { beforeEach, describe, expect, it } from 'vitest';
import { env, SELF } from 'cloudflare:test';
import { flagBucket } from '../src/lib/feature-flags';
import type { FeatureFlag, User } from '../../shared-web/types';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  markUserAdmin,
  signupAndGetCookie,
} from './helpers';

beforeEach(clearDB);

async function adminCookie() {
  const { cookie, userId } = await signupAndGetCookie('admin@example.com');
  await markUserAdmin(userId);
  return cookie;
}

function putFlag(cookie: string, key: string, body: unknown) {
  return SELF.fetch(`${BASE}/admin/flags/${key}`, {
    method: 'PUT',
    headers: authHeaders(cookie),
    body: JSON.stringify(body),
  });
}

function putOverride(cookie: string, key: string, email: string, enabled: boolean | null) {
  return SELF.fetch(`${BASE}/admin/flags/${key}/users`, {
    method: 'PUT',
    headers: authHeaders(cookie),
    body: JSON.stringify({ email, enabled }),
  });
}

async function userFlags(cookie: string) {
  const res = await SELF.fetch(`${BASE}/user`, { headers: authHeaders(cookie) });
  expect(res.status).toBe(200);
  return ((await res.json()) as User).feature_flags;
}

async function deviceFlags(refreshToken: string) {
  const res = await SELF.fetch(`${BASE}/d/device`, {
    headers: { Authorization: `Bearer ${refreshToken}` },
  });
  expect(res.status).toBe(200);
  return ((await res.json()) as { feature_flags: string[] }).feature_flags;
}

describe('flagBucket (API-061)', () => {
  it('matches the API-061 formula', async () => {
    // uint32_be(sha256("beta:<id>")[0..4]) % 10000, computed independently.
    expect(await flagBucket('beta', '00000000-0000-4000-8000-000000000000')).toBe(7899);
    expect(await flagBucket('beta', '11111111-1111-4111-8111-111111111111')).toBe(3911);
  });

  it('gives each flag its own cohort', async () => {
    const userIds = Array.from(
      { length: 50 },
      (_, i) => `00000000-0000-4000-8000-${String(i).padStart(12, '0')}`,
    );
    const a = await Promise.all(userIds.map((id) => flagBucket('a', id)));
    const b = await Promise.all(userIds.map((id) => flagBucket('b', id)));
    expect(a).not.toEqual(b);
    for (const bucket of [...a, ...b]) {
      expect(bucket).toBeGreaterThanOrEqual(0);
      expect(bucket).toBeLessThan(10000);
    }
  });
});

describe('Feature flag admin routes', () => {
  it('allow PUT from the web app in a CORS preflight (API-063)', async () => {
    const res = await SELF.fetch(`${BASE}/admin/flags/beta`, {
      method: 'OPTIONS',
      headers: {
        Origin: new URL(env.APP_URL).origin,
        'Access-Control-Request-Method': 'PUT',
      },
    });
    expect(res.headers.get('Access-Control-Allow-Methods')).toContain('PUT');
  });

  it('reject non-admins (API-061)', async () => {
    const { cookie } = await signupAndGetCookie('user@example.com');
    const res = await SELF.fetch(`${BASE}/admin/flags`, { headers: authHeaders(cookie) });
    expect(res.status).toBe(403);
    expect((await putFlag(cookie, 'beta', { rollout_percent: 100 })).status).toBe(403);
  });

  it('create, list, and update a flag, keeping the description when omitted (API-063, API-062)', async () => {
    const cookie = await adminCookie();

    const created = await putFlag(cookie, 'beta', { rollout_percent: 10, description: 'Beta' });
    expect(created.status).toBe(200);
    const flag = (await created.json()) as FeatureFlag;
    expect(flag).toMatchObject({
      key: 'beta',
      rollout_percent: 10,
      description: 'Beta',
      overrides: [],
    });
    expect(flag.created_at).toBe(flag.updated_at);

    const updated = (await (await putFlag(cookie, 'beta', { rollout_percent: 50 })).json()) as
      | FeatureFlag
      | undefined;
    expect(updated).toMatchObject({ rollout_percent: 50, description: 'Beta' });

    await putFlag(cookie, 'alpha', { rollout_percent: 0 });
    const list = (await (
      await SELF.fetch(`${BASE}/admin/flags`, { headers: authHeaders(cookie) })
    ).json()) as FeatureFlag[];
    expect(list.map((f) => f.key)).toEqual(['alpha', 'beta']);
  });

  it('validate the key and the percentage (API-063)', async () => {
    const cookie = await adminCookie();
    expect((await putFlag(cookie, 'Bad-Key', { rollout_percent: 10 })).status).toBe(400);
    expect((await putFlag(cookie, 'beta', { rollout_percent: 101 })).status).toBe(400);
    expect((await putFlag(cookie, 'beta', { rollout_percent: 2.5 })).status).toBe(400);
  });

  it('404 for an unknown flag or user (API-064, API-065)', async () => {
    const cookie = await adminCookie();
    expect((await putOverride(cookie, 'beta', 'admin@example.com', true)).status).toBe(404);
    const deleted = await SELF.fetch(`${BASE}/admin/flags/beta`, {
      method: 'DELETE',
      headers: authHeaders(cookie),
    });
    expect(deleted.status).toBe(404);

    await putFlag(cookie, 'beta', { rollout_percent: 0 });
    expect((await putOverride(cookie, 'beta', 'nobody@example.com', true)).status).toBe(404);
  });
});

describe('Feature flag evaluation (API-061)', () => {
  it('is empty when no flags exist', async () => {
    const { cookie } = await signupAndGetCookie('user@example.com');
    expect(await userFlags(cookie)).toEqual([]);
  });

  it('follows rollout_percent at 0, 100, and in between', async () => {
    const admin = await adminCookie();
    const { cookie, userId } = await signupAndGetCookie('user@example.com');

    await putFlag(admin, 'beta', { rollout_percent: 100 });
    expect(await userFlags(cookie)).toEqual(['beta']);

    await putFlag(admin, 'beta', { rollout_percent: 0 });
    expect(await userFlags(cookie)).toEqual([]);

    await putFlag(admin, 'beta', { rollout_percent: 50 });
    const expected = (await flagBucket('beta', userId)) < 5000 ? ['beta'] : [];
    expect(await userFlags(cookie)).toEqual(expected);
  });

  it('lets an override win over the percentage in either direction (API-065)', async () => {
    const admin = await adminCookie();
    const { cookie } = await signupAndGetCookie('user@example.com');

    await putFlag(admin, 'beta', { rollout_percent: 0 });
    const res = await putOverride(admin, 'beta', 'User@Example.com', true);
    expect(res.status).toBe(200);
    const flag = (await res.json()) as FeatureFlag;
    expect(flag.overrides).toEqual([
      { user_id: expect.any(String), email: 'user@example.com', enabled: true },
    ]);
    expect(await userFlags(cookie)).toEqual(['beta']);

    await putFlag(admin, 'beta', { rollout_percent: 100 });
    await putOverride(admin, 'beta', 'user@example.com', false);
    expect(await userFlags(cookie)).toEqual([]);

    const cleared = (await (
      await putOverride(admin, 'beta', 'user@example.com', null)
    ).json()) as FeatureFlag;
    expect(cleared.overrides).toEqual([]);
    expect(await userFlags(cookie)).toEqual(['beta']);
  });

  it('drops overrides when the flag is deleted (API-064)', async () => {
    const admin = await adminCookie();
    const { cookie } = await signupAndGetCookie('user@example.com');

    await putFlag(admin, 'beta', { rollout_percent: 0 });
    await putOverride(admin, 'beta', 'user@example.com', true);
    const deleted = await SELF.fetch(`${BASE}/admin/flags/beta`, {
      method: 'DELETE',
      headers: authHeaders(admin),
    });
    expect(deleted.status).toBe(204);
    expect(await userFlags(cookie)).toEqual([]);

    // A new flag with the same key starts with no overrides.
    const recreated = (await (
      await putFlag(admin, 'beta', { rollout_percent: 0 })
    ).json()) as FeatureFlag;
    expect(recreated.overrides).toEqual([]);
    expect(await userFlags(cookie)).toEqual([]);
  });

  it("gives devices and GET /data the owner's flags", async () => {
    const admin = await adminCookie();
    const { cookie } = await signupAndGetCookie('user@example.com');
    const device = await createDeviceForUser('user@example.com');

    await putFlag(admin, 'beta', { rollout_percent: 0 });
    await putOverride(admin, 'beta', 'user@example.com', true);

    expect(await deviceFlags(device.refresh_token)).toEqual(['beta']);
    const data = (await (
      await SELF.fetch(`${BASE}/data`, { headers: authHeaders(cookie) })
    ).json()) as { user: User };
    expect(data.user.feature_flags).toEqual(['beta']);
    // The admin has no override, so the flag stays off for them.
    expect(await userFlags(admin)).toEqual([]);
  });
});
