import { Hono } from 'hono';
import { authenticateWebSession } from '../middleware/auth';
import { requireAdmin } from '../middleware/admin';
import { validateZ } from '../middleware/validation';
import { listAnalyticsSnapshots } from '../lib/db';
import { snapshotAnalytics } from '../lib/analytics';
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

export default admin;
