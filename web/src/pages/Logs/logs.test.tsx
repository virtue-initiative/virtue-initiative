import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { renderWithClient } from '../../test-utils';
import { TEST_DEVICES } from '../../mocks/fixtures';
import type { FeedLog } from './types';
import { Logs } from './index';
import { LOG_CATEGORIES } from './shared';

const SAMPLE_LOGS: FeedLog[] = [
  {
    id: 'log-login',
    device_id: TEST_DEVICES[0].id,
    ts: Date.now(),
    created_at: Date.now(),
    type: 'lifecycle',
    data: { kind: 'system_login', utc_ms: Date.now() },
    batch_status: 'verified',
    source: 'batch',
  },
];

vi.mock('../../utils/cache/client', () => ({
  cacheClient: {
    setSession: vi.fn(),
    cacheQuery: (
      _query: unknown,
      cb: (update: {
        logs: FeedLog[];
        replace: boolean;
        done: boolean;
        processed: number;
        total: number;
      }) => void,
    ) => {
      // Real cache client delivers updates over a BroadcastChannel/worker, so the
      // callback always fires asynchronously — mirror that here, since calling it
      // synchronously would race the effect's own `setLogResult(initial)` call.
      Promise.resolve().then(() => {
        cb({ logs: SAMPLE_LOGS, replace: true, done: true, processed: 1, total: 1 });
      });
    },
    refetch: vi.fn(),
    clearCache: vi.fn().mockResolvedValue(undefined),
    deleteDeviceData: vi.fn().mockResolvedValue(undefined),
    getEventImage: vi.fn().mockResolvedValue(null),
    getDeviceBatchEndTimes: vi.fn().mockResolvedValue([]),
    refetchUpdates: vi.fn().mockResolvedValue(undefined),
    subscribeUpdates: vi.fn().mockReturnValue(() => {}),
    setUnauthorizedHandler: vi.fn(),
  },
}));

// NOTE: The SQLite/OPFS cache worker is not available in the happy-dom test environment.
// The component handles a null cacheClient gracefully (cacheQuery is a no-op), so
// the UI renders correctly with empty log lists.

describe('Logs — header', () => {
  it('shows "My logs" heading by default', async () => {
    renderWithClient(<Logs />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /^my logs$/i, level: 1 })).toBeInTheDocument();
    });
  });
});

describe('Logs — device dropdown', () => {
  it('shows device names in the device dropdown after devices load', async () => {
    renderWithClient(<Logs />);
    await waitFor(() => {
      expect(screen.getByRole('option', { name: TEST_DEVICES[0].name })).toBeInTheDocument();
      expect(screen.getByRole('option', { name: TEST_DEVICES[1].name })).toBeInTheDocument();
    });
  });

  it('selecting a device updates the heading', async () => {
    const user = userEvent.setup();
    renderWithClient(<Logs />);

    // Wait for device options to appear, then get the parent select
    const deviceOpts = await screen.findAllByRole('option', { name: TEST_DEVICES[0].name });
    const deviceSelect = deviceOpts[0].closest('select')!;

    await user.selectOptions(deviceSelect, [TEST_DEVICES[0].name]);

    await waitFor(() => {
      expect(
        screen.getByRole('heading', { name: new RegExp(TEST_DEVICES[0].name, 'i'), level: 1 }),
      ).toBeInTheDocument();
    });
  });
});

describe('Logs — filter panel', () => {
  it('renders the filter toggle button', async () => {
    renderWithClient(<Logs />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /edit search/i })).toBeInTheDocument();
    });
  });

  it('opens the filter panel on click and shows risk select', async () => {
    const user = userEvent.setup();
    renderWithClient(<Logs />);

    const filterBtn = await screen.findByRole('button', { name: /edit search/i });
    await user.click(filterBtn);

    await waitFor(() => {
      expect(screen.getByText(/search filters/i)).toBeInTheDocument();
    });
  });
});

describe('Logs — view switching', () => {
  it('shows List and Gallery navigation links', async () => {
    renderWithClient(<Logs />);
    await waitFor(() => {
      expect(screen.getByRole('link', { name: /list/i })).toBeInTheDocument();
      expect(screen.getByRole('link', { name: /gallery/i })).toBeInTheDocument();
    });
  });
});

describe('Logs — type filter', () => {
  it('offers kind/reason-specific categories, not just generic types', async () => {
    renderWithClient(<Logs />);
    await waitFor(() => {
      expect(screen.getByRole('option', { name: 'System Login' })).toBeInTheDocument();
      expect(screen.getByRole('option', { name: 'Suspend Detected' })).toBeInTheDocument();
      expect(screen.getByRole('option', { name: 'Heartbeat' })).toBeInTheDocument();
    });
    expect(LOG_CATEGORIES).toContain('System Login');
  });

  it('matches a lifecycle log against its specific kind category, not the generic type', async () => {
    // NOTE: LogsList virtualizes rows via @tanstack/react-virtual, which doesn't paint
    // items in happy-dom (no real layout/ResizeObserver signal), so we can't assert on
    // rendered row text here. Instead we assert on the "No logs found." empty state,
    // which the list only shows when the *filtered* item count is truly zero — a
    // faithful proxy for whether the type filter matched.
    const user = userEvent.setup();
    renderWithClient(<Logs />);

    await waitFor(() => {
      expect(screen.queryByText('No logs found.')).not.toBeInTheDocument();
    });

    const typeSelect = screen
      .getByRole('option', { name: 'System Login' })
      .closest('select') as HTMLSelectElement;

    // Regression for the `{ ...item, data: {} }` bug: stripping `data` made every
    // lifecycle log resolve to the generic "Activity" category, so a specific-kind
    // filter like "System Login" would never match and the log would disappear.
    await user.selectOptions(typeSelect, 'System Login');
    await waitFor(() => {
      expect(screen.queryByText('No logs found.')).not.toBeInTheDocument();
    });

    await user.selectOptions(typeSelect, 'Suspend Detected');
    await waitFor(() => {
      expect(screen.getByText('No logs found.')).toBeInTheDocument();
    });
  });
});
