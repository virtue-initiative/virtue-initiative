// What the Logs page tells a partner about the trustworthiness of what they read: which
// events are unverified (WEB-017), how concerning each one is (WEB-024), and what the
// Decryption stats dialog says about batches that could not be read (WEB-018). The cache
// is replaced by a fake that serves fixed events; the decryption itself is proven against
// real batches in utils/cache/decryption.test.ts.
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { renderWithClient } from '../../test-utils';
import { TEST_DEVICES } from '../../mocks/fixtures';
import type { DecryptionStats } from '../../utils/cache/client';
import type { FeedLog } from './types';
import { Logs } from './index';
import { LogDetailDialog } from './shared';

let SAMPLE_LOGS: FeedLog[] = [];
let STATS: { global: DecryptionStats; filtered: DecryptionStats };
const getDecryptionStats = vi.fn();
const resetCache = vi.fn();

vi.mock('../../utils/cache/client', () => ({
  cacheClient: {
    setSession: vi.fn(),
    cacheQuery: (_query: unknown, cb: (update: unknown) => void) => {
      Promise.resolve().then(() =>
        cb({ logs: SAMPLE_LOGS, replace: true, done: true, processed: 1, total: 1 }),
      );
    },
    refetch: vi.fn(),
    clearCache: vi.fn().mockResolvedValue(undefined),
    resetCache: (...args: unknown[]) => resetCache(...args),
    getDecryptionStats: (...args: unknown[]) => getDecryptionStats(...args),
    getEventImage: vi.fn().mockResolvedValue(null),
    getDeviceBatchEndTimes: vi.fn().mockResolvedValue([]),
  },
}));

// Virtualised rows only paint when their scroll container has a size, and happy-dom has
// no layout.
beforeAll(() => {
  for (const prop of ['offsetHeight', 'offsetWidth', 'clientHeight', 'clientWidth']) {
    Object.defineProperty(HTMLElement.prototype, prop, { configurable: true, value: 800 });
  }
  HTMLElement.prototype.getBoundingClientRect = () =>
    ({ width: 800, height: 800, top: 0, left: 0, right: 800, bottom: 800, x: 0, y: 0 }) as DOMRect;
});

afterEach(() => {
  SAMPLE_LOGS = [];
  getDecryptionStats.mockReset();
  resetCache.mockReset();
  window.history.replaceState({}, '', '/');
});

function event(id: string, overrides: Partial<FeedLog> = {}): FeedLog {
  return {
    id,
    device_id: TEST_DEVICES[0].id,
    ts: Date.now() - 60_000,
    created_at: Date.now(),
    type: 'alert',
    data: { message: `message ${id}` },
    batch_status: 'verified',
    source: 'batch',
    ...overrides,
  };
}

/** The list row for the event whose message is `message`. */
function row(message: string) {
  return screen.getByRole('button', { name: new RegExp(message) });
}

async function renderList(logs: FeedLog[]) {
  SAMPLE_LOGS = logs;
  window.history.replaceState({}, '', '/logs/list');
  renderWithClient(<Logs />);
  await screen.findByText('Logs synced');
}

describe('Logs list — unverified events (WEB-017)', () => {
  it('marks events from a batch whose hash chain does not match as "⚠ Unverified", and only those (WEB-017)', async () => {
    await renderList([
      event('trusted'),
      event('tampered', { batch_status: 'failed' }),
      event('not-checked', { batch_status: 'unknown' }),
    ]);
    expect(within(row('message tampered')).getByText('⚠ Unverified')).toBeInTheDocument();
    expect(within(row('message trusted')).queryByText('⚠ Unverified')).not.toBeInTheDocument();
    expect(within(row('message not-checked')).queryByText('⚠ Unverified')).not.toBeInTheDocument();
  });

  it('still shows the unverified event, with its message (WEB-017)', async () => {
    await renderList([event('tampered', { batch_status: 'failed' })]);
    expect(row('message tampered')).toBeInTheDocument();
    expect(screen.queryByText('No logs found.')).not.toBeInTheDocument();
  });

  it('marks the event "⚠ Unverified" in the detail dialog it opens (WEB-017)', async () => {
    const user = userEvent.setup();
    await renderList([event('tampered', { batch_status: 'failed' }), event('trusted')]);

    await user.click(row('message tampered'));
    expect(within(await screen.findByRole('dialog')).getByText('⚠ Unverified')).toBeInTheDocument();
  });
});

describe('Log detail dialog — unverified events (WEB-017)', () => {
  function renderDialog(item: FeedLog) {
    return render(
      <LogDetailDialog
        item={item}
        deviceName={() => TEST_DEVICES[0].name}
        onClose={() => {}}
        viewerId="user-1"
      />,
    );
  }

  it('shows "⚠ Unverified" for an event from a batch that failed verification (WEB-017)', () => {
    renderDialog(event('tampered', { batch_status: 'failed' }));
    expect(screen.getByText('⚠ Unverified')).toBeInTheDocument();
  });

  it.each(['verified', 'unknown'] as const)(
    'shows no unverified mark for a %s event (WEB-017)',
    (batch_status) => {
      renderDialog(event('fine', { batch_status }));
      expect(screen.queryByText('⚠ Unverified')).not.toBeInTheDocument();
    },
  );
});

describe('Concern levels (WEB-024)', () => {
  // [score, percentage shown, level]. The level is judged on the rounded percentage.
  const LEVELS: [number, string, string][] = [
    [0.01, '1%', 'Low'],
    [0.39, '39%', 'Low'],
    [0.394, '39%', 'Low'],
    [0.395, '40%', 'Medium'],
    [0.4, '40%', 'Medium'],
    [0.69, '69%', 'Medium'],
    [0.694, '69%', 'Medium'],
    [0.695, '70%', 'High'],
    [0.7, '70%', 'High'],
    [0.89, '89%', 'High'],
    [0.894, '89%', 'High'],
    [0.895, '90%', 'Alert'],
    [0.8999999999999999, '90%', 'Alert'],
    [0.9, '90%', 'Alert'],
    [1, '100%', 'Alert'],
  ];

  it.each(LEVELS)(
    'the detail dialog reads "%s -> %s concern (%s)" (WEB-024)',
    (risk, percent, level) => {
      render(
        <LogDetailDialog
          item={event('scored', { risk })}
          deviceName={() => TEST_DEVICES[0].name}
          onClose={() => {}}
          viewerId="user-1"
        />,
      );
      expect(
        screen.getByText(new RegExp(`${percent} concern \\(${level}\\)$`)),
      ).toBeInTheDocument();
    },
  );

  it('the detail dialog says "Concern unavailable" for an event with no score (WEB-024)', () => {
    render(
      <LogDetailDialog
        item={event('unscored')}
        deviceName={() => TEST_DEVICES[0].name}
        onClose={() => {}}
        viewerId="user-1"
      />,
    );
    expect(screen.getByText('Concern unavailable')).toBeInTheDocument();
  });

  it('list badges read "⚠ Alert", "⚠ High" and "Med", and Low has none (WEB-024)', async () => {
    await renderList([
      event('alert', { risk: 0.95 }),
      event('high', { risk: 0.75 }),
      event('medium', { risk: 0.5 }),
      event('low', { risk: 0.2 }),
      event('none'),
    ]);
    expect(within(row('message alert')).getByText('⚠ Alert')).toBeInTheDocument();
    expect(within(row('message high')).getByText('⚠ High')).toBeInTheDocument();
    expect(within(row('message medium')).getByText('Med')).toBeInTheDocument();
    for (const id of ['low', 'none']) {
      const r = row(`message ${id}`);
      for (const badge of ['⚠ Alert', '⚠ High', 'Med']) {
        expect(within(r).queryByText(badge)).not.toBeInTheDocument();
      }
    }
  });

  it.each([
    [0.395, 'Med'],
    [0.69, 'Med'],
    [0.695, '⚠ High'],
    [0.894, '⚠ High'],
    [0.895, '⚠ Alert'],
  ])(
    'the list badge for a score of %s is "%s", judged on the rounded percentage (WEB-024)',
    async (risk, badge) => {
      await renderList([event('edge', { risk })]);
      expect(within(row('message edge')).getByText(badge)).toBeInTheDocument();
    },
  );

  it('no badge for a score that rounds below 40% (WEB-024)', async () => {
    await renderList([event('edge', { risk: 0.394 })]);
    const r = row('message edge');
    for (const badge of ['⚠ Alert', '⚠ High', 'Med']) {
      expect(within(r).queryByText(badge)).not.toBeInTheDocument();
    }
  });
});

describe('Decryption stats dialog (WEB-018)', () => {
  const emptyStats: DecryptionStats = {
    totalBatches: 0,
    decryptedBatches: 0,
    failedBatches: 0,
    failureReasons: [],
    totalEvents: 0,
    totalScreenshots: 0,
  };

  async function openStats(global: DecryptionStats, filtered: DecryptionStats = global) {
    STATS = { global, filtered };
    // First call is "All time, all devices" (no filter), second is "Current filter".
    getDecryptionStats.mockImplementation(
      (_viewer: string, _target: string, deviceId?: string, start?: number) =>
        Promise.resolve(
          deviceId !== undefined || start !== undefined ? STATS.filtered : STATS.global,
        ),
    );
    const user = userEvent.setup();
    renderWithClient(<Logs />);
    await screen.findByText('Logs synced');
    await user.click(screen.getByRole('button', { name: 'Decryption stats' }));
    const heading = await screen.findByText('Decryption stats');
    return { user, dialog: heading.closest('dialog') as HTMLElement };
  }

  function section(title: string) {
    return screen.getByRole('heading', { name: title }).parentElement as HTMLElement;
  }

  it('shows batches decrypted out of the total, failures, events and screenshots for all time and for the current filter (WEB-018)', async () => {
    await openStats(
      {
        totalBatches: 12,
        decryptedBatches: 10,
        failedBatches: 0,
        failureReasons: [],
        totalEvents: 480,
        totalScreenshots: 37,
      },
      {
        totalBatches: 3,
        decryptedBatches: 3,
        failedBatches: 0,
        failureReasons: [],
        totalEvents: 90,
        totalScreenshots: 8,
      },
    );
    await waitFor(() => expect(section('All time, all devices')).toBeInTheDocument());
    const all = within(section('All time, all devices'));
    expect(all.getByText('Batches decrypted').nextElementSibling).toHaveTextContent('10 / 12');
    expect(all.getByText('Batches that failed to decrypt').nextElementSibling).toHaveTextContent(
      '0',
    );
    expect(all.getByText('Total events').nextElementSibling).toHaveTextContent('480');
    expect(all.getByText('Total screenshots').nextElementSibling).toHaveTextContent('37');

    const filtered = within(section('Current filter'));
    expect(filtered.getByText('Batches decrypted').nextElementSibling).toHaveTextContent('3 / 3');
    expect(filtered.getByText('Total events').nextElementSibling).toHaveTextContent('90');
    expect(filtered.getByText('Total screenshots').nextElementSibling).toHaveTextContent('8');
  });

  it('asks for the unfiltered stats and for the stats of the current device and date range (WEB-018)', async () => {
    window.history.replaceState({}, '', `/logs?device_id=${TEST_DEVICES[0].id}`);
    await openStats(emptyStats);
    await waitFor(() => expect(getDecryptionStats).toHaveBeenCalledTimes(2));
    const [unfiltered, filtered] = getDecryptionStats.mock.calls;
    expect(unfiltered.slice(2)).toEqual([]);
    expect(filtered[2]).toBe(TEST_DEVICES[0].id);
    expect(filtered[3]).toBeLessThan(filtered[4]);
  });

  it('lists each distinct error with its count and a "Why?" link to the decryption errors help page (WEB-018)', async () => {
    const { user } = await openStats({
      totalBatches: 9,
      decryptedBatches: 4,
      failedBatches: 5,
      failureReasons: [
        { error: 'The operation failed for an operation-specific reason', count: 3 },
        { error: 'Batch blob too short for AES-GCM payload', count: 2 },
      ],
      totalEvents: 50,
      totalScreenshots: 5,
    });
    const all = within(await waitFor(() => section('All time, all devices')));
    expect(all.getByText('Batches that failed to decrypt').closest('summary')).toHaveTextContent(
      '5',
    );

    // The list is behind the failed-batches row.
    await user.click(all.getByText('Batches that failed to decrypt'));
    const list = all.getByRole('list');
    const items = within(list).getAllByRole('listitem');
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveTextContent('The operation failed for an operation-specific reason ×3');
    expect(items[1]).toHaveTextContent('Batch blob too short for AES-GCM payload ×2');
    for (const item of items) {
      const why = within(item).getByRole('link', { name: 'Why?' });
      expect(why.getAttribute('href')).toMatch(/\/help\/web\/decryption-errors$/);
      expect(why).toHaveAttribute('target', '_blank');
    }
  });

  it('offers no error list when nothing failed (WEB-018)', async () => {
    await openStats({
      ...emptyStats,
      totalBatches: 2,
      decryptedBatches: 2,
      totalEvents: 4,
    });
    await waitFor(() => section('All time, all devices'));
    expect(screen.queryByRole('link', { name: 'Why?' })).not.toBeInTheDocument();
    expect(screen.queryByRole('list')).not.toBeInTheDocument();
  });

  it('offers Clear cache, which clears without asking for confirmation (WEB-018, WEB-013)', async () => {
    resetCache.mockResolvedValue(undefined);
    const { user, dialog } = await openStats(emptyStats);
    await waitFor(() => section('All time, all devices'));
    const clear = within(dialog).getByRole('button', { name: 'Clear cache' });
    await user.click(clear);
    expect(resetCache).toHaveBeenCalledTimes(1);
    expect(screen.queryByText(/are you sure/i)).not.toBeInTheDocument();
  });
});
