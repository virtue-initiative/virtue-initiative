import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { renderWithClient } from '../../test-utils';
import { TEST_DEVICES } from '../../mocks/fixtures';
import type { FeedLog } from './types';
import { Logs } from './index';
import { LOG_CATEGORIES, LogDetailDialog } from './shared';

const DEFAULT_SAMPLE_LOGS: FeedLog[] = [
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

// Reassignable per test (see "Logs — skipped screenshots filter" below), and
// read lazily by the mocked `cacheQuery` below, so a test can swap in its own
// fixture without disturbing the others. Reset after every test.
let SAMPLE_LOGS: FeedLog[] = DEFAULT_SAMPLE_LOGS;
afterEach(() => {
  SAMPLE_LOGS = DEFAULT_SAMPLE_LOGS;
  // `useUrlState` persists filters into `window.location` via
  // `history.replaceState`, which — unlike component state — isn't torn down
  // between tests by @testing-library's auto-cleanup. Without this, a filter
  // set by one test (e.g. the "type filter" tests below) leaks into the next
  // test's initial render.
  window.history.replaceState({}, '', window.location.pathname);
});

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
      expect(screen.getByRole('option', { name: 'Daily Check-in' })).toBeInTheDocument();
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

describe('Logs — skipped screenshots filter', () => {
  const skippedLog: FeedLog = {
    id: 'log-skipped',
    device_id: TEST_DEVICES[0].id,
    ts: Date.now(),
    created_at: Date.now(),
    type: 'screenshot_skipped',
    data: { reason: 'locked_or_screensaver' },
    batch_status: 'verified',
    source: 'batch',
  };

  it('hides skipped screenshots by default', async () => {
    SAMPLE_LOGS = [skippedLog];
    renderWithClient(<Logs />);
    await waitFor(() => {
      expect(screen.getByText('No logs found.')).toBeInTheDocument();
    });
  });

  it('shows skipped screenshots once "Show skipped screenshots" is checked', async () => {
    SAMPLE_LOGS = [skippedLog];
    const user = userEvent.setup();
    renderWithClient(<Logs />);

    await waitFor(() => {
      expect(screen.getByText('No logs found.')).toBeInTheDocument();
    });

    const [checkbox] = screen.getAllByRole('checkbox', { name: /show skipped screenshots/i });
    await user.click(checkbox);

    await waitFor(() => {
      expect(screen.queryByText('No logs found.')).not.toBeInTheDocument();
    });
  });

  it('always shows skipped screenshots when explicitly filtering to that type', async () => {
    SAMPLE_LOGS = [skippedLog];
    const user = userEvent.setup();
    renderWithClient(<Logs />);

    await waitFor(() => {
      expect(screen.getByText('No logs found.')).toBeInTheDocument();
    });

    const typeSelect = screen
      .getByRole('option', { name: 'System Login' })
      .closest('select') as HTMLSelectElement;
    await user.selectOptions(typeSelect, 'Screenshot Skipped');

    await waitFor(() => {
      expect(screen.queryByText('No logs found.')).not.toBeInTheDocument();
    });
  });
});

describe('Logs — URL-driven filters (#659)', () => {
  it('clears the device filter when the URL loses device_id', async () => {
    window.history.replaceState({}, '', `/logs?device_id=${TEST_DEVICES[0].id}`);
    renderWithClient(<Logs />);

    await waitFor(() => {
      expect(
        screen.getByRole('heading', { name: new RegExp(TEST_DEVICES[0].name, 'i'), level: 1 }),
      ).toBeInTheDocument();
    });

    // What clicking "My logs" in the sidebar does: preact-iso's LocationProvider
    // navigates by listening for popstate.
    window.history.pushState({}, '', '/logs');
    window.dispatchEvent(new PopStateEvent('popstate'));

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /^my logs$/i, level: 1 })).toBeInTheDocument();
    });
  });
});

describe('Logs — gallery is the default view (#661)', () => {
  it('shows non-image logs and the type filter in the default (gallery) view', async () => {
    window.history.replaceState({}, '', '/logs');
    renderWithClient(<Logs />);

    // DEFAULT_SAMPLE_LOGS holds a single lifecycle log with no image — before
    // #661 the gallery dropped it and fell back to the empty state.
    await waitFor(() => {
      expect(screen.getByRole('option', { name: 'System Login' })).toBeInTheDocument();
    });
    expect(screen.queryByText('No logs found.')).not.toBeInTheDocument();
  });
});

describe('LogDetailDialog — prev/next navigation (#660)', () => {
  const item = DEFAULT_SAMPLE_LOGS[0];

  // Virtualised rows don't paint in happy-dom (see the note above), so drive the
  // dialog directly rather than opening it from the gallery.
  function renderDialog(onPrev: () => void, onNext: () => void) {
    return render(
      <LogDetailDialog
        item={item}
        deviceName={() => TEST_DEVICES[0].name}
        onClose={() => {}}
        viewerId="user-1"
        onPrev={onPrev}
        onNext={onNext}
      />,
    );
  }

  it('steps with the arrow keys', async () => {
    const user = userEvent.setup();
    const onPrev = vi.fn();
    const onNext = vi.fn();
    renderDialog(onPrev, onNext);

    await user.keyboard('{ArrowRight}');
    expect(onNext).toHaveBeenCalledTimes(1);

    await user.keyboard('{ArrowLeft}');
    expect(onPrev).toHaveBeenCalledTimes(1);
  });

  it('drops the previous screenshot when stepping onto a log without one', async () => {
    const imageItem: FeedLog = {
      ...item,
      id: 'log-screenshot',
      type: 'screenshot',
      data: { image: [82, 73, 70, 70] },
      image_w: 100,
      image_h: 50,
    };

    const { rerender } = render(
      <LogDetailDialog
        item={imageItem}
        deviceName={() => TEST_DEVICES[0].name}
        onClose={() => {}}
        viewerId="user-1"
      />,
    );
    await waitFor(() => {
      expect(screen.getByAltText('screenshot')).toBeInTheDocument();
    });

    rerender(
      <LogDetailDialog
        item={item}
        deviceName={() => TEST_DEVICES[0].name}
        onClose={() => {}}
        viewerId="user-1"
      />,
    );
    await waitFor(() => {
      expect(screen.queryByAltText('screenshot')).not.toBeInTheDocument();
    });
  });

  it('steps when the on-screen arrows are clicked', async () => {
    const user = userEvent.setup();
    const onPrev = vi.fn();
    const onNext = vi.fn();
    renderDialog(onPrev, onNext);

    await user.click(screen.getByRole('button', { name: /next log/i }));
    expect(onNext).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole('button', { name: /previous log/i }));
    expect(onPrev).toHaveBeenCalledTimes(1);
  });
});
