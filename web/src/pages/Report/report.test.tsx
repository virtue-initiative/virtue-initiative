import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { renderWithClient } from '../../test-utils';
import { TEST_DEVICES, TEST_WATCHING } from '../../mocks/fixtures';
import type { FeedLog } from '../Logs/types';
import { Report } from './index';

const queries: { startTime?: number; endTime?: number; userId: string }[] = [];
let SAMPLE_LOGS: FeedLog[] = [];

afterEach(() => {
  SAMPLE_LOGS = [];
  queries.length = 0;
  window.history.replaceState({}, '', window.location.pathname);
});

vi.mock('../../utils/cache/client', () => ({
  cacheClient: {
    setSession: vi.fn(),
    cacheQuery: (
      query: { startTime?: number; endTime?: number; userId: string },
      cb: (update: {
        logs: FeedLog[];
        replace: boolean;
        done: boolean;
        processed: number;
        total: number;
      }) => void,
    ) => {
      queries.push(query);
      Promise.resolve().then(() => {
        cb({ logs: SAMPLE_LOGS, replace: true, done: true, processed: 1, total: 1 });
      });
    },
    refetch: vi.fn(),
    clearCache: vi.fn().mockResolvedValue(undefined),
    getEventImage: vi.fn().mockResolvedValue(null),
    getDeviceBatchEndTimes: vi.fn().mockResolvedValue([]),
  },
}));

function log(id: string, type: string, risk: number, data: FeedLog['data'] = {}): FeedLog {
  const ts = Date.now() - 60_000;
  return {
    id,
    device_id: TEST_DEVICES[0].id,
    ts,
    created_at: ts,
    type,
    data,
    risk,
    batch_status: 'verified',
    source: 'batch',
  };
}

describe('Report', () => {
  it('summarizes flagged activity for today', async () => {
    SAMPLE_LOGS = [
      log('stop', 'user_stop', 0.9),
      log('shot-1', 'screenshot', 0.95),
      log('shot-2', 'screenshot', 0.75),
      log('shot-3', 'screenshot', 0.5),
      log('shot-4', 'screenshot', 0),
    ];
    renderWithClient(<Report />);

    expect(await screen.findByRole('heading', { name: /^my report$/i, level: 1 })).toBeVisible();
    await waitFor(() => {
      expect(
        screen.getByText(/there was 1 high risk alert and 2 high risk screenshots today\./i),
      ).toBeInTheDocument();
    });
    expect(screen.getByText(/you have 2 devices being monitored\./i)).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'High risk alerts' })).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'High risk screenshots' })).toBeInTheDocument();
    expect(
      screen.getByText(/there was also 1 medium risk alert or screenshot\./i),
    ).toBeInTheDocument();
    expect(screen.getByText('Monitoring Stopped by User')).toBeInTheDocument();
  });

  it('shows an all-clear message when nothing was flagged', async () => {
    SAMPLE_LOGS = [log('shot', 'screenshot', 0.1)];
    renderWithClient(<Report />);

    expect(await screen.findByText(/nothing concerning was flagged today\./i)).toBeInTheDocument();
    expect(screen.queryByRole('region', { name: /risk/i })).not.toBeInTheDocument();
  });

  it('treats unchanged-screen skips as activity, not missing data', async () => {
    SAMPLE_LOGS = [log('skip', 'screenshot_skipped', 0, { reason: 'static_screen' })];
    renderWithClient(<Report />);

    expect(await screen.findByText(/nothing concerning was flagged today\./i)).toBeInTheDocument();
    expect(screen.queryByText(/no screen activity was recorded/i)).not.toBeInTheDocument();
  });

  it('says there is no data, without an all-clear, when nothing was captured', async () => {
    SAMPLE_LOGS = [log('locked', 'screenshot_skipped', 0, { reason: 'locked_or_screensaver' })];
    renderWithClient(<Report userId={TEST_WATCHING.user.id} />);

    expect(await screen.findByText(/no screen activity was recorded today\./i)).toBeInTheDocument();
    expect(screen.getByText(/was using a device during this time, reach out/i)).toBeInTheDocument();
    expect(screen.queryByText(/nothing concerning was flagged/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/note of encouragement/i)).not.toBeInTheDocument();
  });

  it('names the watched person on their report', async () => {
    renderWithClient(<Report userId={TEST_WATCHING.user.id} />);
    expect(
      await screen.findByRole('heading', { name: `${TEST_WATCHING.user.name}'s report` }),
    ).toBeInTheDocument();
  });

  it('refuses a report for someone the viewer does not monitor', async () => {
    renderWithClient(<Report userId="stranger" />);
    expect(await screen.findByText(/this report isn't available\./i)).toBeInTheDocument();
  });

  it('widens the query to seven days for the weekly view', async () => {
    renderWithClient(<Report />);
    await waitFor(() => expect(queries.length).toBeGreaterThan(0));
    const daily = queries[queries.length - 1];

    await userEvent.click(screen.getByRole('button', { name: 'Weekly' }));
    await waitFor(() => {
      const weekly = queries[queries.length - 1];
      expect(weekly.endTime).toBe(daily.endTime);
      expect(weekly.startTime).toBe(
        new Date(daily.startTime!).setDate(new Date(daily.startTime!).getDate() - 6),
      );
    });
    expect(screen.getByRole('heading', { name: /activity report for the week of/i })).toBeVisible();
  });

  it('jumps to a date chosen in the date picker', async () => {
    const { container } = renderWithClient(<Report />);
    await screen.findByRole('button', { name: /choose a date/i });

    const input = container.querySelector<HTMLInputElement>('input[type="date"]')!;
    fireEvent.change(input, { target: { value: '2026-03-14' } });

    expect(
      await screen.findByRole('heading', { name: 'March 14th, 2026 activity report' }),
    ).toBeVisible();
    await waitFor(() => {
      expect(queries[queries.length - 1].startTime).toBe(new Date(2026, 2, 14).getTime());
    });
  });
});
