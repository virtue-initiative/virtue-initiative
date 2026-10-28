import { Hono } from 'hono';
import { z } from 'zod';
import { authenticateWebSession } from '../middleware/auth';
import { requireAdmin } from '../middleware/admin';
import { validateZ } from '../middleware/validation';
import {
  deleteFeatureFlag,
  listAnalyticsSnapshots,
  setUserFlagOverride,
  upsertFeatureFlag,
} from '../lib/db';
import { snapshotAnalytics } from '../lib/analytics';
import { clearFeatureFlagCache, listFeatureFlagsWithOverrides } from '../lib/feature-flags';
import {
  FullScanError,
  InvalidAdminQueryError,
  listAdminQueryPresets,
  runPresetQuery,
  runReadOnlyQuery,
} from '../lib/admin-queries';
import { Env, Variables } from '../types/bindings';
import {
  adminQuerySchema,
  featureFlagKeySchema,
  setFeatureFlagOverrideSchema,
  upsertFeatureFlagSchema,
  type AnalyticsMetrics,
  type AnalyticsSnapshot,
} from '../../../shared-web/types';

const admin = new Hono<{ Bindings: Env; Variables: Variables }>();

admin.use('/*', authenticateWebSession(), requireAdmin());

const MAX_SNAPSHOT_DAYS = 365;
const DEFAULT_SNAPSHOT_DAYS = 90;

// API-052
admin.get('/analytics', async (c) => {
  const requested = Number.parseInt(c.req.query('days') ?? '', 10);
  const days = Number.isFinite(requested)
    ? Math.min(Math.max(requested, 1), MAX_SNAPSHOT_DAYS)
    : DEFAULT_SNAPSHOT_DAYS;

  const rows = await listAnalyticsSnapshots(c.env.DB, days);
  const snapshots: AnalyticsSnapshot[] = rows.map((row) => ({
    day: row.day,
    metrics: JSON.parse(row.metrics) as AnalyticsMetrics,
    created_at: row.created_at,
  }));
  return c.json(snapshots);
});

// API-053
admin.post('/analytics/refresh', async (c) => {
  const snapshot = await snapshotAnalytics(c.env, Date.now());
  return c.json(snapshot);
});

// API-054
admin.get('/query/presets', (c) => c.json(listAdminQueryPresets()));

// API-055
admin.post('/query', validateZ('json', adminQuerySchema), async (c) => {
  const body = c.req.valid('json');

  try {
    if ('preset' in body) {
      const result = await runPresetQuery(c.env.DB, body.preset, body.limit);
      if (!result) {
        return c.json({ error: 'Not found' }, 404);
      }
      return c.json(result);
    }
    return c.json(
      await runReadOnlyQuery(c.env.DB, body.sql, {
        limit: body.limit,
        allowScan: body.allow_scan,
      }),
    );
  } catch (error) {
    if (error instanceof FullScanError) {
      return c.json(
        {
          error: 'Query would scan a large table',
          details: { code: 'full_scan', tables: error.tables, plan: error.plan },
        },
        400,
      );
    }
    if (error instanceof InvalidAdminQueryError) {
      return c.json({ error: 'Invalid query', details: { message: error.message } }, 400);
    }
    const message = error instanceof Error ? error.message : String(error);
    return c.json({ error: 'Query failed', details: { message } }, 400);
  }
});

const flagKeyParamSchema = z.object({ key: featureFlagKeySchema });

async function findFeatureFlag(db: D1Database, key: string) {
  return (await listFeatureFlagsWithOverrides(db)).find((flag) => flag.key === key) ?? null;
}

// API-062
admin.get('/flags', async (c) => c.json(await listFeatureFlagsWithOverrides(c.env.DB)));

// API-063
admin.put(
  '/flags/:key',
  validateZ('param', flagKeyParamSchema),
  validateZ('json', upsertFeatureFlagSchema),
  async (c) => {
    const { key } = c.req.valid('param');
    const { rollout_percent, description } = c.req.valid('json');
    await upsertFeatureFlag(c.env.DB, { key, rollout_percent, description, now: Date.now() });
    clearFeatureFlagCache();
    return c.json(await findFeatureFlag(c.env.DB, key));
  },
);

// API-064
admin.delete('/flags/:key', validateZ('param', flagKeyParamSchema), async (c) => {
  const { key } = c.req.valid('param');
  if (!(await deleteFeatureFlag(c.env.DB, key))) {
    return c.json({ error: 'Not found' }, 404);
  }
  clearFeatureFlagCache();
  return c.body(null, 204);
});

// API-065
admin.put(
  '/flags/:key/users',
  validateZ('param', flagKeyParamSchema),
  validateZ('json', setFeatureFlagOverrideSchema),
  async (c) => {
    const { key } = c.req.valid('param');
    const { email, enabled } = c.req.valid('json');
    if (!(await findFeatureFlag(c.env.DB, key))) {
      return c.json({ error: 'Flag not found' }, 404);
    }
    const updated = await setUserFlagOverride(c.env.DB, email.trim().toLowerCase(), key, enabled);
    if (!updated) {
      return c.json({ error: 'User not found' }, 404);
    }
    return c.json(await findFeatureFlag(c.env.DB, key));
  },
);

export default admin;
