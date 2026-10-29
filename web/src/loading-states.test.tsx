// WEB-009: a view that loads data shows "Loading…", then either a message saying there is
// nothing or the content, and never the "nothing" message while it is still loading.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, screen, waitFor } from '@testing-library/preact';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { server } from './mocks/server';
import { TEST_ANALYTICS_SNAPSHOT, TEST_DEVICES, TEST_LOCKED_PASSWORD } from './mocks/fixtures';
import { renderWithClient } from './test-utils';
import type { FeedLog } from './pages/Logs/types';
import { Admin } from './pages/Admin/index';
import { Devices } from './pages/Devices/index';
import { Logs } from './pages/Logs/index';
import { Passwords } from './pages/Passwords/index';
import { Report } from './pages/Report/index';
import { Settings } from './pages/Settings/index';

const API = `http://localhost:8787/${CURRENT_API_VERSION}`;

type Update = {
  logs: FeedLog[];
  replace: boolean;
  done: boolean;
  processed: number;
  total: number;
};
const pendingQueries: ((update: Update) => void)[] = [];

vi.mock('./utils/cache/client', () => ({
  cacheClient: {
    setSession: () => {},
    // The cache answers only when the test says so.
    cacheQuery: (_query: unknown, cb: (update: Update) => void) => {
      pendingQueries.push(cb);
    },
    refetch: () => {},
    clearCache: async () => {},
    getDeviceLastUploads: async () => ({}),
    getDeviceBatchEndTimes: async () => [],
    getDecryptionStats: async () => ({}),
    getEventImage: async () => null,
  },
}));

afterEach(() => {
  pendingQueries.length = 0;
  window.history.replaceState({}, '', '/');
});

/** Holds a GET until `release`, then answers with `body`. */
function slow(path: string, body: unknown) {
  let release: () => void = () => {};
  const gate = new Promise<void>((resolve) => (release = resolve));
  server.use(
    http.get(`${API}${path}`, async () => {
      await gate;
      return HttpResponse.json(body);
    }),
  );
  return release;
}

/** Waits out the app-level "Loading…" splash, which also reads "Loading…". */
async function pageLoading() {
  await waitFor(() => expect(document.querySelector('.splash')).toBeNull());
  return screen.getAllByText('Loading…')[0];
}

async function answerCache(logs: FeedLog[]) {
  await waitFor(() => expect(pendingQueries.length).toBeGreaterThan(0));
  await act(async () => {
    for (const cb of pendingQueries)
      cb({ logs, replace: true, done: true, processed: 1, total: 1 });
  });
}

describe.each([
  {
    page: 'Devices',
    render: () => renderWithClient(<Devices />),
    path: '/device',
    empty: { body: [], message: 'Add your first device' },
    content: { body: TEST_DEVICES, text: TEST_DEVICES[0].name },
  },
  {
    page: 'Passwords',
    render: () => renderWithClient(<Passwords />),
    path: '/locked-password',
    empty: { body: [], message: 'No locked passwords' },
    content: { body: [TEST_LOCKED_PASSWORD], text: TEST_LOCKED_PASSWORD.label },
  },
  {
    page: 'Admin snapshot',
    render: () => renderWithClient(<Admin />),
    path: '/admin/analytics',
    empty: { body: [], message: 'No snapshot yet. Refresh data to compute one now.' },
    content: { body: [TEST_ANALYTICS_SNAPSHOT], text: 'Active users' },
  },
])('$page (WEB-009)', ({ render, path, empty, content }) => {
  it('shows "Loading…" and not the empty message while loading, then the empty message when nothing came back (WEB-009)', async () => {
    const release = slow(path, empty.body);
    render();

    expect(await pageLoading()).toBeInTheDocument();
    expect(screen.queryByText(empty.message)).not.toBeInTheDocument();

    release();
    expect(await screen.findByText(empty.message)).toBeInTheDocument();
    expect(screen.queryByText('Loading…')).not.toBeInTheDocument();
  });

  it('shows "Loading…" and no empty message while loading, then the content (WEB-009)', async () => {
    const release = slow(path, content.body);
    render();

    expect(await pageLoading()).toBeInTheDocument();
    expect(screen.queryByText(empty.message)).not.toBeInTheDocument();

    release();
    expect((await screen.findAllByText(content.text)).length).toBeGreaterThan(0);
    expect(screen.queryByText('Loading…')).not.toBeInTheDocument();
    expect(screen.queryByText(empty.message)).not.toBeInTheDocument();
  });
});

describe('Settings (WEB-009, WEB-058)', () => {
  it('shows "Loading…" instead of an empty form while the user loads (WEB-009)', async () => {
    const release = slow('/user', {
      id: 'user-1',
      email: 'test@example.com',
      email_verified: true,
      email_bounced_at: null,
      name: 'Test User',
      settings: { email_frequency: 'daily', timezone: 'UTC' },
      feature_flags: [],
    });
    renderWithClient(<Settings />);

    expect(await pageLoading()).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Delete account' })).not.toBeInTheDocument();

    release();
    expect(await screen.findByRole('button', { name: 'Delete account' })).toBeInTheDocument();
    expect(screen.queryByText('Loading…')).not.toBeInTheDocument();
  });
});

describe('Logs (WEB-009, WEB-040)', () => {
  const event = (): FeedLog => ({
    id: 'e1',
    device_id: TEST_DEVICES[0].id,
    ts: Date.now() - 60_000,
    created_at: Date.now(),
    type: 'heartbeat',
    data: {},
    batch_status: 'verified',
    source: 'batch',
  });

  it.each(['/logs', '/logs/list'])(
    'at %s, does not say "No logs found." while the first sync is running, and does once it finishes empty (WEB-009)',
    async (path) => {
      window.history.replaceState({}, '', path);
      renderWithClient(<Logs />);

      expect(await screen.findByText('Syncing logs…')).toBeInTheDocument();
      expect(screen.queryByText('No logs found.')).not.toBeInTheDocument();

      await answerCache([]);
      expect(await screen.findByText('No logs found.')).toBeInTheDocument();
      expect(screen.getByText('Logs synced')).toBeInTheDocument();
    },
  );

  it('shows no empty message once logs arrive (WEB-009)', async () => {
    renderWithClient(<Logs />);
    await screen.findByText('Syncing logs…');
    await answerCache([event()]);

    expect(await screen.findByText('Logs synced')).toBeInTheDocument();
    expect(screen.queryByText('No logs found.')).not.toBeInTheDocument();
  });
});

describe('Report (WEB-009, WEB-046)', () => {
  it('says it is syncing, and neither "No screen activity" nor the all-clear, until the logs are in (WEB-009)', async () => {
    renderWithClient(<Report />);

    expect(await screen.findByText('Syncing logs.')).toBeInTheDocument();
    expect(screen.queryByText(/No screen activity was recorded/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Nothing concerning was flagged/)).not.toBeInTheDocument();

    await answerCache([]);
    expect(await screen.findByText(/No screen activity was recorded/)).toBeInTheDocument();
    expect(screen.queryByText('Syncing logs.')).not.toBeInTheDocument();
  });
});
