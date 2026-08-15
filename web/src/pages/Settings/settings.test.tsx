import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { describe, expect, it } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_USER } from '../../mocks/fixtures';
import { renderWithClient } from '../../test-utils';
import { Settings } from './index';

const BASE = `http://localhost:8787/${CURRENT_API_VERSION}`;

describe('Settings — page renders', () => {
  it('shows the user display name', async () => {
    renderWithClient(<Settings />);
    await waitFor(() => {
      expect(screen.getByDisplayValue(TEST_USER.name!)).toBeInTheDocument();
    });
  });

  it('shows the user email', async () => {
    renderWithClient(<Settings />);
    await waitFor(() => {
      expect(screen.getByDisplayValue(TEST_USER.email)).toBeInTheDocument();
    });
  });

  it('renders "Delete account" button', async () => {
    renderWithClient(<Settings />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /delete account/i })).toBeInTheDocument();
    });
  });
});

describe('Settings — save name', () => {
  it('sends PATCH /user with updated name', async () => {
    let patchBody: unknown;
    server.use(
      http.patch(`${BASE}/user`, async ({ request }) => {
        patchBody = await request.json();
        return HttpResponse.json({ email_verification_required: false });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Settings />);

    // Wait for user to load and name field to be populated
    const nameInput = await screen.findByDisplayValue(TEST_USER.name!);
    await user.clear(nameInput);
    await user.type(nameInput, 'New Name');

    const saveButtons = screen.getAllByRole('button', { name: /^save$/i });
    await user.click(saveButtons[0]);

    await waitFor(() => {
      expect((patchBody as { name: string }).name).toBe('New Name');
    });
  });
});

describe('Settings — email frequency', () => {
  it('sends PATCH /user with updated email frequency', async () => {
    let patchBody: unknown;
    server.use(
      http.patch(`${BASE}/user`, async ({ request }) => {
        patchBody = await request.json();
        return HttpResponse.json({ email_verification_required: false });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Settings />);

    // Wait for user data to load
    await screen.findByDisplayValue(TEST_USER.name!);

    const select = screen.getByRole('combobox');
    await user.selectOptions(select, 'weekly');

    const saveButtons = screen.getAllByRole('button', { name: /^save$/i });
    await user.click(saveButtons[0]);

    await waitFor(() => {
      expect(
        (patchBody as { settings: { email_frequency: string } }).settings?.email_frequency,
      ).toBe('weekly');
    });
  });
});

describe('Settings — delete account', () => {
  it('opens delete dialog when "Delete account" is clicked', async () => {
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    const deleteBtn = await screen.findByRole('button', { name: /^delete account$/i });
    await user.click(deleteBtn);

    expect(screen.getByText(/type.*to confirm/i)).toBeInTheDocument();
  });

  it('disables confirm button when email does not match', async () => {
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    const deleteBtn = await screen.findByRole('button', { name: /^delete account$/i });
    await user.click(deleteBtn);

    // The confirm button should be disabled until email matches
    const confirmBtns = screen.getAllByRole('button', { name: /^delete account$/i });
    // The dialog's delete button is the last one
    const confirmBtn = confirmBtns[confirmBtns.length - 1];
    expect(confirmBtn).toBeDisabled();
  });

  it('calls DELETE /user after typing matching email', async () => {
    let deleteCalled = false;
    server.use(
      http.delete(`${BASE}/user`, () => {
        deleteCalled = true;
        return new HttpResponse(null, { status: 204 });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Settings />);

    const deleteBtn = await screen.findByRole('button', { name: /^delete account$/i });
    await user.click(deleteBtn);

    // Type the matching email in the confirmation input
    const emailInput = screen.getByPlaceholderText(TEST_USER.email);
    await user.type(emailInput, TEST_USER.email);

    const confirmBtns = screen.getAllByRole('button', { name: /^delete account$/i });
    const confirmBtn = confirmBtns[confirmBtns.length - 1];
    expect(confirmBtn).not.toBeDisabled();
    await user.click(confirmBtn);

    await waitFor(() => {
      expect(deleteCalled).toBe(true);
    });
  });
});
