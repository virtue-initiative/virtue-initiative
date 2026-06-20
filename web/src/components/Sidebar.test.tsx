import { screen, waitFor } from '@testing-library/preact';
import { describe, expect, it } from 'vitest';
import { TEST_WATCHING } from '../mocks/fixtures';
import { renderWithClient } from '../test-utils';
import { Sidebar } from './Sidebar';

describe('Sidebar', () => {
  it('renders the primary nav links and user account button', async () => {
    renderWithClient(<Sidebar />);

    await waitFor(() => {
      expect(screen.getByRole('link', { name: /^devices/i })).toBeInTheDocument();
    });
    expect(screen.getByRole('link', { name: /^partners/i })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /my logs/i })).toBeInTheDocument();
    // Settings and Log out are in the user account popup; verify the trigger button exists
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /test user/i })).toBeInTheDocument();
    });
  });

  it('renders a logs sub-item per accepted partner', async () => {
    renderWithClient(<Sidebar />);

    await waitFor(() => {
      expect(
        screen.getByRole('link', { name: new RegExp(`${TEST_WATCHING.user.name} logs`, 'i') }),
      ).toBeInTheDocument();
    });
    expect(
      screen.getByRole('link', { name: new RegExp(`${TEST_WATCHING.user.name} logs`, 'i') }),
    ).toHaveAttribute('href', `/logs/${TEST_WATCHING.user.id}`);
  });
});
