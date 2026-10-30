import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_ANALYTICS_SNAPSHOT, TEST_FEATURE_FLAG } from '../../mocks/fixtures';
import { renderWithClient } from '../../test-utils';
import { Admin } from './index';

const BASE = `http://localhost:8787/${CURRENT_API_VERSION}`;

describe('Admin', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    window.localStorage.clear();
  });

  it('renders the 404 page when the API answers 403 (WEB-026)', async () => {
    server.use(
      http.get(`${BASE}/admin/analytics`, () =>
        HttpResponse.json({ error: 'Forbidden' }, { status: 403 }),
      ),
      http.get(`${BASE}/admin/query/presets`, () =>
        HttpResponse.json({ error: 'Forbidden' }, { status: 403 }),
      ),
    );

    renderWithClient(<Admin />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /page not found/i })).toBeInTheDocument();
    });
    expect(screen.queryByRole('heading', { name: /^admin$/i })).not.toBeInTheDocument();
  });

  it('renders stat tiles from the latest snapshot (WEB-066)', async () => {
    renderWithClient(<Admin />);

    expect(await screen.findByRole('heading', { name: /^admin$/i })).toBeInTheDocument();
    await waitFor(() => {
      expect(document.querySelectorAll('.admin-stat')).toHaveLength(7);
    });
    const tiles = document.querySelectorAll('.admin-stat');
    expect(tiles[0]).toHaveTextContent('Users');
    expect(tiles[0]).toHaveTextContent(String(TEST_ANALYTICS_SNAPSHOT.metrics.users.total));
    expect(tiles[4]).toHaveTextContent(String(TEST_ANALYTICS_SNAPSHOT.metrics.batches.total));
    // Ratios are computed from the stored counts: 34 active devices / 17 active users,
    // 63 devices / 42 owners, 24 partners / 20 people.
    expect(tiles[2]).toHaveTextContent('2 per active user');
    expect(tiles[3]).toHaveTextContent('1.5 per person with any');
    expect(tiles[5]).toHaveTextContent('1.2 per person with any');
  });

  it('draws one chart per variable and lets any be hidden or shown (WEB-067)', async () => {
    const user = userEvent.setup();
    renderWithClient(<Admin />);

    await waitFor(() => {
      expect(document.querySelectorAll('.admin-trend').length).toBeGreaterThan(20);
    });
    const count = document.querySelectorAll('.admin-trend').length;
    // Fixed variables plus one per platform in the fixture (android, linux).
    expect(screen.getByRole('img', { name: /^Devices on linux, 30 on/ })).toBeInTheDocument();
    expect(screen.getByRole('img', { name: /^Batches, 9,001 on/ })).toBeInTheDocument();

    await user.click(screen.getByRole('checkbox', { name: 'Batches' }));
    expect(document.querySelectorAll('.admin-trend')).toHaveLength(count - 1);
    expect(screen.queryByRole('img', { name: /^Batches, 9,001 on/ })).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /hide all/i }));
    expect(document.querySelectorAll('.admin-trend')).toHaveLength(0);
    expect(screen.getByText(/every variable is hidden/i)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /show all/i }));
    expect(document.querySelectorAll('.admin-trend')).toHaveLength(count);
  });

  it('offers a table view of the same variables (WEB-067)', async () => {
    const user = userEvent.setup();
    renderWithClient(<Admin />);

    await user.click(await screen.findByRole('button', { name: /^table$/i }));

    expect(screen.getByRole('columnheader', { name: 'Locked passwords' })).toBeInTheDocument();
    expect(screen.getByRole('columnheader', { name: 'Devices per person' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: TEST_ANALYTICS_SNAPSHOT.day })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: '1.50' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: '9,001' })).toBeInTheDocument();
  });

  it('prompts for a refresh when no snapshot exists yet (WEB-066)', async () => {
    server.use(http.get(`${BASE}/admin/analytics`, () => HttpResponse.json([])));

    renderWithClient(<Admin />);

    await waitFor(() => {
      expect(screen.getByText(/no snapshot yet/i)).toBeInTheDocument();
    });
  });

  it('calls the refresh endpoint and shows the new snapshot (WEB-066)', async () => {
    let refreshed = 0;
    server.use(
      http.get(`${BASE}/admin/analytics`, () => HttpResponse.json([])),
      http.post(`${BASE}/admin/analytics/refresh`, () => {
        refreshed += 1;
        return HttpResponse.json({
          ...TEST_ANALYTICS_SNAPSHOT,
          metrics: {
            ...TEST_ANALYTICS_SNAPSHOT.metrics,
            users: { ...TEST_ANALYTICS_SNAPSHOT.metrics.users, total: 77 },
          },
        });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Admin />);

    await user.click(await screen.findByRole('button', { name: /refresh data/i }));

    await waitFor(() => {
      expect(refreshed).toBe(1);
      expect(document.querySelector('.admin-stat')).toHaveTextContent('77');
      expect(screen.getByRole('img', { name: /^Users, 77 on/ })).toBeInTheDocument();
    });
  });

  it('runs a preset and renders the results table (WEB-068)', async () => {
    const user = userEvent.setup();
    renderWithClient(<Admin />);

    const runPreset = await screen.findByRole('button', { name: /run preset/i });
    await waitFor(() => expect(runPreset).toBeEnabled());
    await user.click(runPreset);

    await waitFor(() => {
      expect(screen.getByRole('columnheader', { name: 'email' })).toBeInTheDocument();
      expect(screen.getByRole('cell', { name: 'test@example.com' })).toBeInTheDocument();
    });
  });

  it('downloads the current result as CSV (WEB-068)', async () => {
    const blobs: Blob[] = [];
    // happy-dom lacks the object-URL statics, so define them for this test.
    const createObjectURL = vi.fn((blob: Blob) => {
      blobs.push(blob);
      return 'blob:test';
    });
    const revokeObjectURL = vi.fn();
    vi.stubGlobal('URL', Object.assign(URL, { createObjectURL, revokeObjectURL }));
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {});

    const user = userEvent.setup();
    renderWithClient(<Admin />);

    const runPreset = await screen.findByRole('button', { name: /run preset/i });
    await waitFor(() => expect(runPreset).toBeEnabled());
    await user.click(runPreset);
    await user.click(await screen.findByRole('button', { name: /download csv/i }));

    expect(click).toHaveBeenCalledTimes(1);
    expect(blobs).toHaveLength(1);
    expect(await blobs[0].text()).toBe('email,created_at\r\ntest@example.com,1700000000000\r\n');
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:test');
  });

  it('runs raw SQL with the row limit and flags a truncated result (WEB-068)', async () => {
    let body: unknown;
    server.use(
      http.post(`${BASE}/admin/query`, async ({ request }) => {
        body = await request.json();
        return HttpResponse.json({ columns: ['n'], rows: [[1]], truncated: true, rows_read: 7 });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Admin />);

    const limitInput = await screen.findByLabelText(/row limit/i);
    expect(limitInput).toHaveValue(100);
    await user.clear(limitInput);
    await user.type(limitInput, '25');
    await user.type(screen.getByLabelText(/raw sql/i), 'SELECT 1 AS n');
    await user.click(screen.getByRole('button', { name: /run sql/i }));

    await waitFor(() => {
      expect(body).toEqual({ sql: 'SELECT 1 AS n', limit: 25 });
      expect(screen.getByRole('alert')).toHaveTextContent(/first 25 rows/i);
      expect(screen.getByRole('heading', { name: /7 rows read/i })).toBeInTheDocument();
    });
  });

  it('warns about an expensive result (WEB-068)', async () => {
    server.use(
      http.post(`${BASE}/admin/query`, () =>
        HttpResponse.json({ columns: ['n'], rows: [[1]], truncated: false, rows_read: 250_000 }),
      ),
    );

    const user = userEvent.setup();
    renderWithClient(<Admin />);

    const runPreset = await screen.findByRole('button', { name: /run preset/i });
    await waitFor(() => expect(runPreset).toBeEnabled());
    await user.click(runPreset);

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent(/read 250,000 rows/i);
    });
  });

  it('holds a full-scan query for confirmation and reruns it with allow_scan (WEB-068)', async () => {
    const bodies: unknown[] = [];
    server.use(
      http.post(`${BASE}/admin/query`, async ({ request }) => {
        const body = (await request.json()) as { allow_scan?: boolean };
        bodies.push(body);
        if (!body.allow_scan) {
          return HttpResponse.json(
            {
              error: 'Query would scan a large table',
              details: { code: 'full_scan', tables: ['batches'], plan: ['SCAN batches'] },
            },
            { status: 400 },
          );
        }
        return HttpResponse.json({ columns: ['id'], rows: [], truncated: false, rows_read: 0 });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Admin />);

    await user.type(await screen.findByLabelText(/raw sql/i), 'SELECT * FROM batches');
    await user.click(screen.getByRole('button', { name: /run sql/i }));

    const runAnyway = await screen.findByRole('button', { name: /run anyway/i });
    expect(screen.getByRole('alert')).toHaveTextContent(/every row of batches/i);
    expect(screen.getByText('SCAN batches')).toBeInTheDocument();

    await user.click(runAnyway);

    await waitFor(() => {
      expect(bodies).toHaveLength(2);
      expect(bodies[1]).toEqual({ sql: 'SELECT * FROM batches', limit: 100, allow_scan: true });
      expect(screen.queryByRole('button', { name: /run anyway/i })).not.toBeInTheDocument();
      expect(screen.getByText('No rows.')).toBeInTheDocument();
    });
  });

  it('lists feature flags with their rollout and overrides (WEB-069)', async () => {
    renderWithClient(<Admin />);

    expect(await screen.findByRole('heading', { name: TEST_FEATURE_FLAG.key })).toBeInTheDocument();
    expect(screen.getByText('25% of users')).toBeInTheDocument();
    expect(screen.getByText('tester@example.com')).toBeInTheDocument();
    expect(
      screen.getByLabelText('Description', { selector: '#flag-new_report_tab-description' }),
    ).toHaveValue(TEST_FEATURE_FLAG.description);
  });

  it('creates a flag (WEB-069)', async () => {
    let body: unknown;
    let key: unknown;
    server.use(
      http.put(`${BASE}/admin/flags/:key`, async ({ params, request }) => {
        key = params.key;
        body = await request.json();
        return HttpResponse.json({
          ...TEST_FEATURE_FLAG,
          key: params.key,
          rollout_percent: 10,
          description: 'Beta',
          overrides: [],
        });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Admin />);

    await user.type(await screen.findByLabelText(/new flag key/i), 'beta');
    const percent = screen.getByLabelText('Rollout %', { selector: '#admin-flag-percent' });
    await user.clear(percent);
    await user.type(percent, '10');
    await user.type(
      screen.getByLabelText('Description', { selector: '#admin-flag-description' }),
      'Beta',
    );
    await user.click(screen.getByRole('button', { name: /create flag/i }));

    await waitFor(() => {
      expect(key).toBe('beta');
      expect(body).toEqual({ rollout_percent: 10, description: 'Beta' });
      expect(screen.getByRole('heading', { name: 'beta' })).toBeInTheDocument();
    });
  });

  it('sets and removes a user override (WEB-069)', async () => {
    const bodies: unknown[] = [];
    server.use(
      http.put(`${BASE}/admin/flags/:key/users`, async ({ request }) => {
        bodies.push(await request.json());
        return HttpResponse.json(TEST_FEATURE_FLAG);
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Admin />);

    await user.type(await screen.findByLabelText(/user email/i), 'someone@example.com');
    await user.selectOptions(screen.getByLabelText(/^override$/i), 'off');
    await user.click(screen.getByRole('button', { name: /set override/i }));
    await user.click(
      screen.getByRole('button', { name: /remove override for tester@example.com/i }),
    );

    await waitFor(() => {
      expect(bodies).toEqual([
        { email: 'someone@example.com', enabled: false },
        { email: 'tester@example.com', enabled: null },
      ]);
    });
  });

  it('deletes a flag after confirming (WEB-069, WEB-020)', async () => {
    let deleted: unknown;
    server.use(
      http.delete(`${BASE}/admin/flags/:key`, ({ params }) => {
        deleted = params.key;
        return new HttpResponse(null, { status: 204 });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Admin />);

    await screen.findByRole('heading', { name: TEST_FEATURE_FLAG.key });
    await user.click(screen.getAllByRole('button', { name: /^delete flag$/i })[0]);
    expect(deleted).toBeUndefined();
    const confirm = screen.getAllByRole('button', { name: /^delete flag$/i });
    await user.click(confirm[confirm.length - 1]);

    await waitFor(() => {
      expect(deleted).toBe(TEST_FEATURE_FLAG.key);
      expect(screen.getByText(/no feature flags yet/i)).toBeInTheDocument();
    });
  });
});
