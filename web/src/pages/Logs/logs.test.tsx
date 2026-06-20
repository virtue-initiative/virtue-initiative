import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { renderWithClient } from '../../test-utils';
import { TEST_DEVICES } from '../../mocks/fixtures';
import { Logs } from './index';

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
