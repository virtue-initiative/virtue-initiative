// Property tests for API-061 (feature flag rollout). The example-based tests are in
// feature-flags.test.ts.
import { beforeEach, describe, expect, it } from 'vitest';
import { env } from 'cloudflare:test';
import fc from 'fast-check';
import { sha256 } from '@noble/hashes/sha2.js';
import { clearFeatureFlagCache, enabledFeatureFlags, flagBucket } from '../src/lib/feature-flags';
import { upsertFeatureFlag } from '../src/lib/db';
import { clearDB } from './helpers';

beforeEach(clearDB);

const keyArb = fc.stringMatching(/^[a-z0-9_]{1,64}$/);
const userIdArb = fc.uuid().map((id) => id.toLowerCase());
const usersArb = fc.uniqueArray(userIdArb, { minLength: 1, maxLength: 25 });
const percentArb = fc.integer({ min: 0, max: 100 });
// Each property writes to D1 on every case, so keep the case count modest.
const runs = { numRuns: 25 };

// Every case starts with no flags. Cases of one property share a database, so a flag left by
// an earlier case would make a later case's "key that names no flag" name a flag.
async function resetFlags() {
  await env.DB.prepare('DELETE FROM feature_flags').run();
  clearFeatureFlagCache();
}

async function setRollout(key: string, rolloutPercent: number) {
  await upsertFeatureFlag(env.DB, { key, rollout_percent: rolloutPercent, now: 1 });
  clearFeatureFlagCache();
}

async function enabledUsers(key: string, users: string[], overrides = new Map<string, boolean>()) {
  const enabled: string[] = [];
  for (const user of users) {
    if ((await enabledFeatureFlags(env.DB, user, overrides)).includes(key)) enabled.push(user);
  }
  return enabled;
}

describe('feature flag buckets (API-061)', () => {
  it('equal uint32_be(sha256(key:user_id)[0..4]) % 10000', async () => {
    await fc.assert(
      fc.asyncProperty(keyArb, userIdArb, async (key, userId) => {
        const digest = sha256(new TextEncoder().encode(`${key}:${userId}`));
        const expected = new DataView(digest.buffer, digest.byteOffset).getUint32(0) % 10000;
        expect(await flagBucket(key, userId)).toBe(expected);
      }),
    );
  });

  it('depend only on the key and the user id, and are below 10000', async () => {
    await fc.assert(
      fc.asyncProperty(keyArb, userIdArb, async (key, userId) => {
        const bucket = await flagBucket(key, userId);
        expect(await flagBucket(key, userId)).toBe(bucket);
        expect(Number.isInteger(bucket)).toBe(true);
        expect(bucket).toBeGreaterThanOrEqual(0);
        expect(bucket).toBeLessThan(10000);
      }),
    );
  });
});

describe('feature flag rollout (API-061)', () => {
  it('never turns a flag off for a user when rollout_percent is raised', async () => {
    await fc.assert(
      fc.asyncProperty(keyArb, usersArb, percentArb, percentArb, async (key, users, a, b) => {
        await resetFlags();
        const [low, high] = a <= b ? [a, b] : [b, a];
        await setRollout(key, low);
        const atLow = await enabledUsers(key, users);
        await setRollout(key, high);
        const atHigh = await enabledUsers(key, users);
        expect(atHigh).toEqual(expect.arrayContaining(atLow));
      }),
      runs,
    );
  });

  it('is on for nobody at 0% and for everybody at 100%, without overrides', async () => {
    await fc.assert(
      fc.asyncProperty(keyArb, usersArb, async (key, users) => {
        await resetFlags();
        await setRollout(key, 0);
        expect(await enabledUsers(key, users)).toEqual([]);
        await setRollout(key, 100);
        expect(await enabledUsers(key, users)).toEqual(users);
      }),
      runs,
    );
  });

  it('lets an override decide, whatever the rollout_percent', async () => {
    await fc.assert(
      fc.asyncProperty(keyArb, userIdArb, percentArb, fc.boolean(), async (key, user, p, on) => {
        await resetFlags();
        await setRollout(key, p);
        const overrides = new Map([[key, on]]);
        expect((await enabledFeatureFlags(env.DB, user, overrides)).includes(key)).toBe(on);
      }),
      runs,
    );
  });

  it('ignores overrides for keys that name no flag', async () => {
    await fc.assert(
      fc.asyncProperty(
        keyArb,
        keyArb,
        userIdArb,
        percentArb,
        fc.boolean(),
        async (key, other, user, p, on) => {
          await resetFlags();
          fc.pre(key !== other);
          await setRollout(key, p);
          const without = await enabledFeatureFlags(env.DB, user, new Map());
          const withStray = await enabledFeatureFlags(env.DB, user, new Map([[other, on]]));
          expect(withStray).toEqual(without);
          expect(withStray).not.toContain(other);
        },
      ),
      runs,
    );
  });
});
