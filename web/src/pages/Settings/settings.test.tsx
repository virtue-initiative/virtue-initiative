import { screen, waitFor, within } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_USER } from '../../mocks/fixtures';
import { makeFakeSession, renderWithClient } from '../../test-utils';
import { Settings } from './index';

const BASE = `http://localhost:8787/${CURRENT_API_VERSION}`;

const resetCache = vi.fn(() => Promise.resolve());

vi.mock('../../utils/cache/client', () => ({
  cacheClient: {
    setSession: vi.fn(),
    cacheQuery: vi.fn(),
    refetch: vi.fn(),
    clearCache: vi.fn(() => Promise.resolve()),
    resetCache: () => resetCache(),
    deleteDeviceData: vi.fn(() => Promise.resolve()),
    getEventImage: vi.fn(() => Promise.resolve(null)),
    getDeviceBatchEndTimes: vi.fn(() => Promise.resolve([])),
    getDecryptionStats: vi.fn(),
  },
  logCacheQuery: vi.fn(),
  triggerRefetch: vi.fn(),
}));

describe('Settings — page renders', () => {
  it('shows the user display name (WEB-059)', async () => {
    renderWithClient(<Settings />);
    await waitFor(() => {
      expect(screen.getByDisplayValue(TEST_USER.name!)).toBeInTheDocument();
    });
  });

  it('shows the user email (WEB-060)', async () => {
    renderWithClient(<Settings />);
    await waitFor(() => {
      expect(screen.getByDisplayValue(TEST_USER.email)).toBeInTheDocument();
    });
  });

  it('renders "Delete account" button (WEB-063)', async () => {
    renderWithClient(<Settings />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /delete account/i })).toBeInTheDocument();
    });
  });
});

describe('Settings — save name (WEB-059)', () => {
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

describe('Settings — email frequency (WEB-059)', () => {
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

describe('Settings — delete account (WEB-063)', () => {
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

describe('Settings — clear cache (WEB-062)', () => {
  it('clears the cache and reloads the page', async () => {
    const reload = vi.fn();
    const originalReload = window.location.reload;
    Object.defineProperty(window.location, 'reload', { configurable: true, value: reload });

    try {
      const user = userEvent.setup();
      renderWithClient(<Settings />);

      const button = await screen.findByRole('button', { name: /^clear cache$/i });
      await user.click(button);

      await waitFor(() => {
        expect(resetCache).toHaveBeenCalled();
        expect(reload).toHaveBeenCalled();
      });
    } finally {
      Object.defineProperty(window.location, 'reload', {
        configurable: true,
        value: originalReload,
      });
    }
  });
});

describe('Settings — change password (WEB-061)', () => {
  async function fillPasswordForm(current: string, next: string, confirm: string) {
    const user = userEvent.setup();
    await user.type(await screen.findByLabelText('Current password'), current);
    await user.type(screen.getByLabelText('New password'), next);
    await user.type(screen.getByLabelText('Confirm new password'), confirm);
    await user.click(screen.getByRole('button', { name: /change password/i }));
  }

  it('changes the password through the session and clears the form', async () => {
    const session = makeFakeSession();
    renderWithClient(<Settings />, undefined, session);

    await fillPasswordForm('current-pass-1', 'brand-new-pass-1', 'brand-new-pass-1');

    await screen.findByText('Your password was changed.');
    expect(session.changePassword).toHaveBeenCalledWith(
      TEST_USER.email,
      'current-pass-1',
      'brand-new-pass-1',
    );
    expect(screen.getByLabelText('Current password')).toHaveValue('');
    expect(screen.getByLabelText('New password')).toHaveValue('');
  });

  it('does not submit when the new passwords do not match', async () => {
    const session = makeFakeSession();
    renderWithClient(<Settings />, undefined, session);

    await fillPasswordForm('current-pass-1', 'brand-new-pass-1', 'brand-new-pass-2');

    expect(await screen.findByText('Passwords do not match.')).toBeInTheDocument();
    expect(session.changePassword).not.toHaveBeenCalled();
  });

  it('does not submit a new password shorter than the minimum', async () => {
    const session = makeFakeSession();
    renderWithClient(<Settings />, undefined, session);

    await fillPasswordForm('current-pass-1', 'short', 'short');

    expect(session.changePassword).not.toHaveBeenCalled();
    expect(screen.getAllByText(/at least 12 characters/i).length).toBeGreaterThan(0);
  });

  it('shows the error when the current password is wrong', async () => {
    const session = makeFakeSession({
      changePassword: vi.fn().mockRejectedValue(new Error('Current password is incorrect.')),
    });
    renderWithClient(<Settings />, undefined, session);

    await fillPasswordForm('wrong-pass-123', 'brand-new-pass-1', 'brand-new-pass-1');

    expect(await screen.findByText('Current password is incorrect.')).toBeInTheDocument();
  });
});

describe('Settings — DNS check domain (WEB-073)', () => {
  // The saved domain as the mocked API holds it. A successful PATCH updates it like the real API.
  let savedDomain: string | null = null;

  beforeEach(() => {
    savedDomain = null;
  });

  function capturePatches(status = 200) {
    const bodies: Record<string, unknown>[] = [];
    server.use(
      http.get(`${BASE}/user`, () =>
        HttpResponse.json({ ...TEST_USER, dns_filter_domain: savedDomain }),
      ),
      http.patch(`${BASE}/user`, async ({ request }) => {
        const body = (await request.json()) as Record<string, unknown>;
        bodies.push(body);
        if (status !== 200) return HttpResponse.json({ error: 'nope' }, { status });
        if ('dns_filter_domain' in body) savedDomain = body.dns_filter_domain as string | null;
        return HttpResponse.json({ ok: true });
      }),
    );
    return bodies;
  }

  function withSavedDomain() {
    savedDomain = 'block.example.net';
  }

  const checkSwitch = () => screen.findByRole('switch', { name: 'Check my DNS filter' });
  const field = () => screen.queryByLabelText('DNS check domain');
  const saveDomain = () =>
    within(screen.getByLabelText('DNS check domain').closest('form')!).getByRole('button', {
      name: /^save$/i,
    });

  it('starts on with the current domain and explains the check when a domain is saved', async () => {
    withSavedDomain();
    capturePatches();
    renderWithClient(<Settings />);

    expect(await screen.findByDisplayValue('block.example.net')).toBeInTheDocument();
    expect(await checkSwitch()).toBeChecked();
    expect(screen.getByText(/add a domain to your dns filter's block list/i)).toBeInTheDocument();
    expect(screen.getByText(/all your devices check that it stays blocked/i)).toBeInTheDocument();
    expect(saveDomain()).toBeDisabled();
  });

  it('starts off with no field when no domain is saved', async () => {
    renderWithClient(<Settings />);

    await screen.findByDisplayValue(TEST_USER.name!);
    expect(await checkSwitch()).not.toBeChecked();
    expect(field()).not.toBeInTheDocument();
  });

  it('reveals and focuses the field when switched on, and sends nothing yet', async () => {
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    await screen.findByDisplayValue(TEST_USER.name!);
    await user.click(await checkSwitch());

    expect(await screen.findByLabelText('DNS check domain')).toHaveFocus();
    expect(await checkSwitch()).toBeChecked();
    expect(bodies).toHaveLength(0);
  });

  it('saves the domain trimmed and lowercased', async () => {
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    await screen.findByDisplayValue(TEST_USER.name!);
    await user.click(await checkSwitch());
    await user.type(await screen.findByLabelText('DNS check domain'), '  Block.Example.NET ');
    await user.click(saveDomain());

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(bodies[0]).toEqual({ dns_filter_domain: 'block.example.net' });
  });

  it('shows an error and sends nothing for an empty domain', async () => {
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    await screen.findByDisplayValue(TEST_USER.name!);
    await user.click(await checkSwitch());
    await user.type(await screen.findByLabelText('DNS check domain'), ' ');
    await user.click(saveDomain());

    expect(await screen.findByText(/./, { selector: '.vi-field__error' })).toBeInTheDocument();
    expect(screen.getByLabelText('DNS check domain')).toHaveAttribute('aria-invalid', 'true');
    expect(bodies).toHaveLength(0);
  });

  it('shows the validation message and sends nothing for an invalid domain', async () => {
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    await screen.findByDisplayValue(TEST_USER.name!);
    await user.click(await checkSwitch());
    const input = await screen.findByLabelText('DNS check domain');
    await user.type(input, 'https://example.net/path');
    await user.click(saveDomain());

    expect(await screen.findByText(/enter a domain such as example\.net/i)).toBeInTheDocument();
    expect(input).toHaveAttribute('aria-invalid', 'true');
    expect(bodies).toHaveLength(0);
  });

  it('sends null and hides the field when switched off with a saved domain after confirming', async () => {
    withSavedDomain();
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    await screen.findByDisplayValue('block.example.net');
    await user.click(await checkSwitch());
    await user.click(await screen.findByRole('button', { name: 'Turn off' }));

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(bodies[0]).toEqual({ dns_filter_domain: null });
    expect(await screen.findByText('Saved')).toBeInTheDocument();
    expect(field()).not.toBeInTheDocument();
    expect(await checkSwitch()).not.toBeChecked();
  });

  it('returns the switch to on and shows an error when switching off fails', async () => {
    withSavedDomain();
    const bodies = capturePatches(500);
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    await screen.findByDisplayValue('block.example.net');
    await user.click(await checkSwitch());
    await user.click(await screen.findByRole('button', { name: 'Turn off' }));

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(await screen.findByRole('alert')).toBeInTheDocument();
    expect(await checkSwitch()).toBeChecked();
    expect(screen.getByDisplayValue('block.example.net')).toBeInTheDocument();
  });

  const offTitle = 'Turn off the DNS filter check?';
  const changeTitle = 'Change the DNS check domain?';
  const dialogOf = (title: string) =>
    screen.getByRole('heading', { name: title }).closest('dialog')!;
  // The browser fires a cancelable `cancel` event on Escape, then closes the dialog.
  const pressEscape = (dialog: HTMLDialogElement) => {
    if (dialog.dispatchEvent(new Event('cancel', { cancelable: true }))) dialog.close();
  };

  it('asks before switching off with a saved domain, defaults to "Keep it on" and sends nothing yet', async () => {
    withSavedDomain();
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    await screen.findByDisplayValue('block.example.net');
    expect(screen.queryByRole('heading', { name: offTitle })).not.toBeInTheDocument();
    await user.click(await checkSwitch());

    expect(await screen.findByRole('heading', { name: offTitle })).toBeInTheDocument();
    expect(screen.getByText(/talk with them first if you can/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Keep it on' })).toHaveAttribute('autofocus');
    expect(bodies).toHaveLength(0);
  });

  it('keeps the check on and sends nothing on "Keep it on"', async () => {
    withSavedDomain();
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    await screen.findByDisplayValue('block.example.net');
    await user.click(await checkSwitch());
    await user.click(await screen.findByRole('button', { name: 'Keep it on' }));

    expect(screen.queryByRole('heading', { name: offTitle })).not.toBeInTheDocument();
    expect(await checkSwitch()).toBeChecked();
    expect(screen.getByDisplayValue('block.example.net')).toBeInTheDocument();
    expect(bodies).toHaveLength(0);
  });

  it('keeps the check on and sends nothing when the turn-off question is dismissed with Escape', async () => {
    withSavedDomain();
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    await screen.findByDisplayValue('block.example.net');
    await user.click(await checkSwitch());
    await screen.findByRole('heading', { name: offTitle });
    pressEscape(dialogOf(offTitle));

    await waitFor(() =>
      expect(screen.queryByRole('heading', { name: offTitle })).not.toBeInTheDocument(),
    );
    expect(await checkSwitch()).toBeChecked();
    expect(bodies).toHaveLength(0);
  });

  it('asks before sending a domain that replaces the saved one, and sends it on "Change domain"', async () => {
    withSavedDomain();
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    const input = await screen.findByDisplayValue('block.example.net');
    expect(screen.queryByRole('heading', { name: changeTitle })).not.toBeInTheDocument();
    await user.clear(input);
    await user.type(input, 'Other.Example.NET');
    await user.click(saveDomain());

    expect(await screen.findByRole('heading', { name: changeTitle })).toBeInTheDocument();
    expect(
      screen.getByText(/add the new domain to your dns filter\'s block list first/i),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Cancel', hidden: false })).toHaveAttribute(
      'autofocus',
    );
    expect(bodies).toHaveLength(0);

    await user.click(screen.getByRole('button', { name: 'Change domain' }));
    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(bodies[0]).toEqual({ dns_filter_domain: 'other.example.net' });
  });

  it('sends nothing and keeps the typed domain when the change is cancelled or dismissed with Escape', async () => {
    withSavedDomain();
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    const input = await screen.findByDisplayValue('block.example.net');
    await user.clear(input);
    await user.type(input, 'other.example.net');
    await user.click(saveDomain());
    await user.click(await screen.findByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('heading', { name: changeTitle })).not.toBeInTheDocument();

    await user.click(saveDomain());
    await screen.findByRole('heading', { name: changeTitle });
    pressEscape(dialogOf(changeTitle));
    await waitFor(() =>
      expect(screen.queryByRole('heading', { name: changeTitle })).not.toBeInTheDocument(),
    );

    expect(screen.getByLabelText('DNS check domain')).toHaveValue('other.example.net');
    expect(bodies).toHaveLength(0);
  });

  it('shows a validation error before any change question', async () => {
    withSavedDomain();
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    const input = await screen.findByDisplayValue('block.example.net');
    await user.clear(input);
    await user.type(input, 'https://example.net/path');
    await user.click(saveDomain());

    expect(await screen.findByText(/enter a domain such as example\.net/i)).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: changeTitle })).not.toBeInTheDocument();
    expect(bodies).toHaveLength(0);
  });

  it('does not ask when the first domain is saved', async () => {
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    await screen.findByDisplayValue(TEST_USER.name!);
    await user.click(await checkSwitch());
    await user.type(await screen.findByLabelText('DNS check domain'), 'block.example.net');
    await user.click(saveDomain());

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(screen.queryByRole('heading', { name: changeTitle })).not.toBeInTheDocument();
  });

  it('sends nothing when switched off with no saved domain', async () => {
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Settings />);

    await screen.findByDisplayValue(TEST_USER.name!);
    await user.click(await checkSwitch());
    await screen.findByLabelText('DNS check domain');
    await user.click(await checkSwitch());

    expect(field()).not.toBeInTheDocument();
    expect(await checkSwitch()).not.toBeChecked();
    expect(screen.queryByRole('heading', { name: offTitle })).not.toBeInTheDocument();
    expect(bodies).toHaveLength(0);
  });
});
