import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_ANALYTICS_SNAPSHOT } from '../../mocks/fixtures';
import { renderWithClient } from '../../test-utils';
import { Admin } from './index';

const BASE = `http://localhost:8787/${CURRENT_API_VERSION}`;

describe('Admin', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('renders the 404 page when the API answers 403', async () => {
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

  it('renders stat tiles and a history row from the latest snapshot', async () => {
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
    expect(screen.getByRole('cell', { name: TEST_ANALYTICS_SNAPSHOT.day })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: '2' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: '1.5' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: '1.2' })).toBeInTheDocument();
    expect(screen.getByRole('columnheader', { name: 'Locked passwords' })).toBeInTheDocument();
    expect(
      screen.getByRole('cell', {
        name: String(TEST_ANALYTICS_SNAPSHOT.metrics.locked_passwords.total),
      }),
    ).toBeInTheDocument();
  });

  it('prompts for a refresh when no snapshot exists yet', async () => {
    server.use(http.get(`${BASE}/admin/analytics`, () => HttpResponse.json([])));

    renderWithClient(<Admin />);

    await waitFor(() => {
      expect(screen.getByText(/no snapshot yet/i)).toBeInTheDocument();
    });
  });

  it('calls the refresh endpoint and shows the new snapshot', async () => {
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
      expect(screen.getByRole('cell', { name: '77' })).toBeInTheDocument();
    });
  });

  it('runs a preset and renders the results table', async () => {
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

  it('downloads the current result as CSV', async () => {
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

  it('runs raw SQL with the row limit and flags a truncated result', async () => {
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

  it('warns about an expensive result', async () => {
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

  it('holds a full-scan query for confirmation and reruns it with allow_scan', async () => {
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
});
