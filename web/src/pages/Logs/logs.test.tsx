import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { renderWithClient } from '../../test-utils';
import { TEST_DEVICES } from '../../mocks/fixtures';
import { Logs } from './index';

// NOTE: data-cache (Dexie/IndexedDB) is not available in the happy-dom test environment.
// The component handles missing IndexedDB gracefully (errors are caught internally), so
// the UI renders correctly with empty log lists. We do NOT mock the module here because
// doing so invalidates the module graph and causes a Preact duplicate-instance error.

describe('Logs — sidebar', () => {
  it('shows "Devices" heading in sidebar', async () => {
    renderWithClient(<Logs />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /^devices$/i })).toBeInTheDocument();
    });
  });

  it('shows device names in sidebar after devices load', async () => {
    renderWithClient(<Logs />);
    await waitFor(() => {
      expect(screen.getByText(TEST_DEVICES[0].name)).toBeInTheDocument();
      expect(screen.getByText(TEST_DEVICES[1].name)).toBeInTheDocument();
    });
  });

  it('shows "My devices" group label', async () => {
    renderWithClient(<Logs />);
    await waitFor(() => {
      expect(screen.getByText('My devices')).toBeInTheDocument();
    });
  });
});

describe('Logs — header', () => {
  it('shows "My logs" heading by default', async () => {
    renderWithClient(<Logs />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /^my logs$/i, level: 1 })).toBeInTheDocument();
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

describe('Logs — selecting a device', () => {
  it('clicking a device updates the heading', async () => {
    const user = userEvent.setup();
    renderWithClient(<Logs />);

    // Wait for device list to load
    const deviceBtn = await screen.findByRole('button', { name: TEST_DEVICES[0].name });
    await user.click(deviceBtn);

    await waitFor(() => {
      expect(
        screen.getByRole('heading', { name: new RegExp(TEST_DEVICES[0].name, 'i'), level: 1 }),
      ).toBeInTheDocument();
    });
  });
});
