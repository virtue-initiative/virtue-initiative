import { listFeatureFlags, listUserFlagOverrides } from './db';
import type { FeatureFlag } from '../../../shared-web/types';

type FlagRule = { key: string; rollout_percent: number };

// API-061: the flag table may be cached for up to 60 seconds. It's read on
// every GET /user and every device settings response, so without this each of
// those would cost one more query.
const CACHE_TTL_MS = 60_000;
let cached: { rules: FlagRule[]; expiresAt: number } | null = null;

async function loadRules(db: D1Database, now: number) {
  if (cached && cached.expiresAt > now) {
    return cached.rules;
  }
  const rules = (await listFeatureFlags(db)).map(({ key, rollout_percent }) => ({
    key,
    rollout_percent,
  }));
  cached = { rules, expiresAt: now + CACHE_TTL_MS };
  return rules;
}

// Called after an admin route changes a flag, so this isolate sees it at once.
export function clearFeatureFlagCache() {
  cached = null;
}

// API-061: uint32_be(sha256(key ":" user_id)[0..4]) % 10000. Salting with the
// key gives each flag its own cohort; the user id never changes, unlike email.
export async function flagBucket(key: string, userId: string) {
  const digest = await crypto.subtle.digest(
    'SHA-256',
    new TextEncoder().encode(`${key}:${userId.toLowerCase()}`),
  );
  return new DataView(digest).getUint32(0) % 10000;
}

async function isEnabled(rule: FlagRule, userId: string, overrides: Map<string, boolean>) {
  const override = overrides.get(rule.key);
  if (override !== undefined) {
    return override;
  }
  return (await flagBucket(rule.key, userId)) < rule.rollout_percent * 100;
}

// The keys of the flags enabled for a user, for `feature_flags` on `User` and
// `DeviceSettings`. Overrides for keys with no flag are ignored.
export async function enabledFeatureFlags(
  db: D1Database,
  userId: string,
  overrides: Map<string, boolean>,
  now = Date.now(),
) {
  const rules = await loadRules(db, now);
  const enabled = await Promise.all(rules.map((rule) => isEnabled(rule, userId, overrides)));
  return rules.filter((_, index) => enabled[index]).map((rule) => rule.key);
}

// Every flag with its overrides, for the admin routes (API-062 to API-065).
export async function listFeatureFlagsWithOverrides(db: D1Database): Promise<FeatureFlag[]> {
  const [flags, users] = await Promise.all([listFeatureFlags(db), listUserFlagOverrides(db)]);
  return flags.map((flag) => ({
    ...flag,
    overrides: users.flatMap((user) => {
      const enabled = user.feature_flags.get(flag.key);
      return enabled === undefined ? [] : [{ user_id: user.id, email: user.email, enabled }];
    }),
  }));
}
