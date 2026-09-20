import { beforeEach, describe, expect, it } from 'vitest';
import { env, SELF } from 'cloudflare:test';
import { ADMIN_QUERY_PRESETS } from '../src/lib/admin-queries';
import type { AdminQueryResult, AnalyticsSnapshot } from '../../shared-web/types';
import {
  authHeaders,
  BASE,
  clearDB,
  markUserAdmin,
  signupAndGetCookie,
  uuidToBytes,
} from './helpers';

beforeEach(clearDB);

async function adminCookie(email = 'admin@example.com') {
  const { cookie, userId } = await signupAndGetCookie(email);
  await markUserAdmin(userId);
  return { cookie, userId };
}

function query(cookie: string, body: unknown) {
  return SELF.fetch(`${BASE}/admin/query`, {
    method: 'POST',
    headers: authHeaders(cookie),
    body: JSON.stringify(body),
  });
}

describe('Admin access control', () => {
  const routes: [string, string][] = [
    ['GET', '/admin/analytics'],
    ['POST', '/admin/analytics/refresh'],
    ['GET', '/admin/query/presets'],
    ['POST', '/admin/query'],
  ];

  it.each(routes)('%s %s returns 401 without a session', async (method, path) => {
    const res = await SELF.fetch(`${BASE}${path}`, {
      method,
      headers: { 'Content-Type': 'application/json' },
      body: method === 'POST' ? JSON.stringify({ preset: 'recent_signups' }) : undefined,
    });
    expect(res.status).toBe(401);
  });

  it.each(routes)('%s %s returns 403 for a non-admin', async (method, path) => {
    const { cookie } = await signupAndGetCookie('user@example.com');
    const res = await SELF.fetch(`${BASE}${path}`, {
      method,
      headers: authHeaders(cookie),
      body: method === 'POST' ? JSON.stringify({ preset: 'recent_signups' }) : undefined,
    });
    expect(res.status).toBe(403);
    expect(await res.json()).toEqual({ error: 'Forbidden' });
  });

  it.each(routes)('%s %s returns 200 for an admin', async (method, path) => {
    const { cookie } = await adminCookie();
    const res = await SELF.fetch(`${BASE}${path}`, {
      method,
      headers: authHeaders(cookie),
      body: method === 'POST' ? JSON.stringify({ preset: 'recent_signups' }) : undefined,
    });
    expect(res.status).toBe(200);
  });
});

describe('Admin analytics', () => {
  it('starts empty, then refresh writes one row and a second refresh replaces it', async () => {
    const { cookie } = await adminCookie();

    const empty = await SELF.fetch(`${BASE}/admin/analytics`, { headers: authHeaders(cookie) });
    expect(await empty.json()).toEqual([]);

    const first = await SELF.fetch(`${BASE}/admin/analytics/refresh`, {
      method: 'POST',
      headers: authHeaders(cookie),
    });
    expect(first.status).toBe(200);
    const firstBody = (await first.json()) as AnalyticsSnapshot;
    expect(firstBody.day).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(firstBody.metrics.users.total).toBe(1);

    await signupAndGetCookie('second@example.com');
    const second = await SELF.fetch(`${BASE}/admin/analytics/refresh`, {
      method: 'POST',
      headers: authHeaders(cookie),
    });
    const secondBody = (await second.json()) as AnalyticsSnapshot;
    expect(secondBody.day).toBe(firstBody.day);
    expect(secondBody.metrics.users.total).toBe(2);

    const list = await SELF.fetch(`${BASE}/admin/analytics?days=7`, {
      headers: authHeaders(cookie),
    });
    const snapshots = (await list.json()) as AnalyticsSnapshot[];
    expect(snapshots).toHaveLength(1);
    expect(snapshots[0].metrics.users.total).toBe(2);
    expect(snapshots[0].created_at).toBe(secondBody.created_at);
  });

  it('lists snapshots newest first and honours days', async () => {
    const { cookie } = await adminCookie();
    const metrics = JSON.stringify({ users: { total: 0 } });
    await env.DB.batch(
      ['2026-09-01', '2026-09-03', '2026-09-02'].map((day) =>
        env.DB.prepare(
          'INSERT INTO analytics_snapshots (day, metrics, created_at) VALUES (?, ?, ?)',
        ).bind(day, metrics, 1),
      ),
    );

    const res = await SELF.fetch(`${BASE}/admin/analytics?days=2`, {
      headers: authHeaders(cookie),
    });
    const snapshots = (await res.json()) as AnalyticsSnapshot[];
    expect(snapshots.map((s) => s.day)).toEqual(['2026-09-03', '2026-09-02']);
  });
});

describe('Admin queries', () => {
  it('lists presets and every preset executes', async () => {
    const { cookie } = await adminCookie();
    await signupAndGetCookie('someone@example.com');

    const res = await SELF.fetch(`${BASE}/admin/query/presets`, { headers: authHeaders(cookie) });
    const presets = (await res.json()) as { name: string; label: string; description: string }[];
    expect(presets.map((p) => p.name).sort()).toEqual(Object.keys(ADMIN_QUERY_PRESETS).sort());

    for (const preset of presets) {
      const run = await query(cookie, { preset: preset.name });
      expect(run.status, preset.name).toBe(200);
      const body = (await run.json()) as AdminQueryResult;
      expect(Array.isArray(body.columns), preset.name).toBe(true);
      expect(body.truncated).toBe(false);
    }
  });

  it('returns 404 for an unknown preset', async () => {
    const { cookie } = await adminCookie();
    const res = await query(cookie, { preset: 'nope' });
    expect(res.status).toBe(404);
  });

  it('rejects a body with neither preset nor sql', async () => {
    const { cookie } = await adminCookie();
    const res = await query(cookie, {});
    expect(res.status).toBe(400);
  });

  it.each([
    ['DELETE FROM users'],
    ['UPDATE users SET email = 1'],
    ['SELECT 1; DROP TABLE users'],
    ['PRAGMA table_info(users)'],
    ['-- comment only'],
    [''],
  ])('rejects %j before executing it', async (sql) => {
    const { cookie, userId } = await adminCookie();
    const res = await query(cookie, { sql });
    expect(res.status).toBe(400);

    const stillThere = await env.DB.prepare('SELECT email FROM users WHERE id = ?')
      .bind(uuidToBytes(userId))
      .first<{ email: string }>();
    expect(stillThere?.email).toBe('admin@example.com');
  });

  it('accepts SELECT, WITH, comments and a trailing semicolon', async () => {
    const { cookie } = await adminCookie();

    const withRes = await query(cookie, { sql: 'WITH x AS (SELECT 1 AS one) SELECT * FROM x' });
    expect(withRes.status).toBe(200);
    expect(await withRes.json()).toMatchObject({ columns: ['one'], rows: [[1]], truncated: false });

    const commented = await query(cookie, {
      sql: '/* leading */ -- note\n  select email from users order by email;',
    });
    expect(commented.status).toBe(200);
    expect(await commented.json()).toMatchObject({
      columns: ['email'],
      rows: [['admin@example.com']],
      truncated: false,
    });
  });

  it('returns 400 with the database message for a query D1 rejects', async () => {
    const { cookie } = await adminCookie();
    const res = await query(cookie, { sql: 'SELECT * FROM no_such_table' });
    expect(res.status).toBe(400);
    const body = (await res.json()) as { error: string; details: { message: string } };
    expect(body.error).toBe('Query failed');
    expect(body.details.message).toMatch(/no_such_table/);
  });

  const SEQ_600 = `WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 600)
                   SELECT n FROM seq`;

  it('truncates at 100 rows by default, even when the query asks for more', async () => {
    const { cookie } = await adminCookie();
    const res = await query(cookie, { sql: `${SEQ_600} LIMIT 300` });
    expect(res.status).toBe(200);
    const body = (await res.json()) as AdminQueryResult;
    expect(body.rows).toHaveLength(100);
    expect(body.truncated).toBe(true);
  });

  it('honours an explicit limit up to 500 and rejects larger ones', async () => {
    const { cookie } = await adminCookie();

    const small = (await (
      await query(cookie, { sql: SEQ_600, limit: 5 })
    ).json()) as AdminQueryResult;
    expect(small.rows.map((r) => r[0])).toEqual([1, 2, 3, 4, 5]);
    expect(small.truncated).toBe(true);

    const max = (await (
      await query(cookie, { sql: SEQ_600, limit: 500 })
    ).json()) as AdminQueryResult;
    expect(max.rows).toHaveLength(500);
    expect(max.truncated).toBe(true);

    const exact = (await (
      await query(cookie, { sql: 'SELECT 1', limit: 1 })
    ).json()) as AdminQueryResult;
    expect(exact.rows).toHaveLength(1);
    expect(exact.truncated).toBe(false);

    expect((await query(cookie, { sql: SEQ_600, limit: 501 })).status).toBe(400);
    expect((await query(cookie, { preset: 'recent_signups', limit: 0 })).status).toBe(400);
  });

  it('strips credential columns and renders BLOB ids as UUIDs', async () => {
    const { cookie, userId } = await adminCookie();

    const res = await query(cookie, { sql: 'SELECT * FROM users' });
    expect(res.status).toBe(200);
    const body = (await res.json()) as AdminQueryResult;
    expect(body.columns).toContain('email');
    expect(body.columns).toContain('id');
    expect(body.columns).not.toContain('password_hash');
    expect(body.columns).not.toContain('password_salt');
    expect(body.columns).not.toContain('encrypted_priv_key');
    expect(body.rows[0]).toHaveLength(body.columns.length);
    expect(body.rows[0][body.columns.indexOf('id')]).toBe(userId);

    const sessions = await query(cookie, { sql: 'SELECT * FROM user_sessions' });
    const sessionBody = (await sessions.json()) as AdminQueryResult;
    expect(sessionBody.columns).not.toContain('refresh_token_hash');
    expect(sessionBody.columns).toContain('user_id');
    expect(sessionBody.rows[0][sessionBody.columns.indexOf('user_id')]).toBe(userId);
  });
});

describe('Admin query cost guard', () => {
  it('reports rows_read on every result', async () => {
    const { cookie } = await adminCookie();
    await signupAndGetCookie('other@example.com');

    const raw = (await (
      await query(cookie, { sql: 'SELECT email FROM users' })
    ).json()) as AdminQueryResult;
    expect(raw.rows_read).toBe(2);

    const preset = (await (
      await query(cookie, { preset: 'recent_signups' })
    ).json()) as AdminQueryResult;
    expect(typeof preset.rows_read).toBe('number');
  });

  it.each([
    ['SELECT * FROM batches'],
    ['SELECT b.id FROM batches b JOIN users u ON u.id = b.user_id'],
    ['SELECT user_id, COUNT(*) AS n FROM batches GROUP BY user_id'],
    ['select id from BATCHES as bb order by end_time'],
  ])('refuses a full scan of batches: %s', async (sql) => {
    const { cookie } = await adminCookie();
    const res = await query(cookie, { sql });
    expect(res.status).toBe(400);
    const body = (await res.json()) as {
      error: string;
      details: { code: string; tables: string[]; plan: string[] };
    };
    expect(body.error).toBe('Query would scan a large table');
    expect(body.details.code).toBe('full_scan');
    expect(body.details.tables).toEqual(['batches']);
    expect(body.details.plan.some((line) => line.startsWith('SCAN'))).toBe(true);
  });

  it('allows indexed lookups on batches, scans of small tables, and allow_scan', async () => {
    const { cookie } = await adminCookie();

    const indexed = await query(cookie, {
      sql: 'SELECT b.id FROM batches b WHERE b.created_at > 0',
    });
    expect(indexed.status).toBe(200);

    const small = await query(cookie, { sql: 'SELECT id, email FROM users ORDER BY email' });
    expect(small.status).toBe(200);

    const forced = await query(cookie, { sql: 'SELECT * FROM batches', allow_scan: true });
    expect(forced.status).toBe(200);
    expect(await forced.json()).toMatchObject({ rows: [], truncated: false });
  });

  it('presets skip the plan check', async () => {
    const { cookie } = await adminCookie();
    const res = await query(cookie, { preset: 'batches_per_day' });
    expect(res.status).toBe(200);
  });
});
