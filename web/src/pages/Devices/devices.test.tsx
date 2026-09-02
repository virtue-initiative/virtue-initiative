import { screen, waitFor, within } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { describe, expect, it } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_DEVICES } from '../../mocks/fixtures';
import { renderWithClient } from '../../test-utils';
import { Devices } from './index';

const BASE = `http://localhost:8787/${CURRENT_API_VERSION}`;

describe('Devices — device list', () => {
  it('renders device names once loaded', async () => {
    renderWithClient(<Devices />);
    await waitFor(() => {
      expect(screen.getByText('My Laptop')).toBeInTheDocument();
      expect(screen.getByText('My Phone')).toBeInTheDocument();
    });
  });

  it('shows "Devices" page heading', async () => {
    renderWithClient(<Devices />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /^devices$/i })).toBeInTheDocument();
    });
  });

  it('shows a loading indicator instead of "No devices" while the fetch is in flight', async () => {
    let resolveDevices: (() => void) | undefined;
    server.use(
      http.get(
        `${BASE}/device`,
        () =>
          new Promise((resolve) => {
            resolveDevices = () => resolve(HttpResponse.json(TEST_DEVICES));
          }),
      ),
    );

    renderWithClient(<Devices />);

    expect(screen.getByText(/loading/i)).toBeInTheDocument();
    expect(screen.queryByText('No devices')).not.toBeInTheDocument();

    await waitFor(() => expect(resolveDevices).toBeDefined());
    resolveDevices?.();

    await waitFor(() => {
      expect(screen.getByText('My Laptop')).toBeInTheDocument();
    });
  });

  it('shows "No devices" once loaded with an empty list', async () => {
    server.use(http.get(`${BASE}/device`, () => HttpResponse.json([])));

    renderWithClient(<Devices />);

    await waitFor(() => {
      expect(screen.getByText('No devices')).toBeInTheDocument();
    });
  });
});

describe('Devices — Add device dialog', () => {
  it('opens the Add device dialog', async () => {
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    const addBtn = await screen.findByRole('button', { name: /add device/i });
    await user.click(addBtn);

    expect(
      screen.getByRole('heading', { name: /enter device code/i, level: 3 }),
    ).toBeInTheDocument();
  });

  it('opens the dialog on load when the URL carries ?add', async () => {
    // The clients print a `/devices?add` deep link; following it should land on
    // the code box rather than on a page with a button still to find.
    const original = window.location.href;
    window.history.replaceState({}, '', '/devices?add');
    try {
      renderWithClient(<Devices />);
      expect(
        await screen.findByRole('heading', { name: /enter device code/i, level: 3 }),
      ).toBeInTheDocument();
    } finally {
      window.history.replaceState({}, '', original);
    }
  });

  it('prefills the code when ?add carries one', async () => {
    // The clients link `?add=<code>`, so the user should not have to read the
    // code off the device and type it in again.
    const original = window.location.href;
    window.history.replaceState({}, '', '/devices?add=K7R-M3X');
    try {
      renderWithClient(<Devices />);
      const input = await screen.findByLabelText('Device code');
      await waitFor(() => expect(input).toHaveValue('K7R-M3X'));
      expect(screen.getByRole('button', { name: /continue/i })).toBeEnabled();
    } finally {
      window.history.replaceState({}, '', original);
    }
  });

  it('ignores an ?add code that is not six characters', async () => {
    // Half a code looks like a typo the user has to find and fix; an empty box
    // is clearer.
    const original = window.location.href;
    window.history.replaceState({}, '', '/devices?add=K7R');
    try {
      renderWithClient(<Devices />);
      const input = await screen.findByLabelText('Device code');
      // The dialog still opens, so wait for that before reading the box.
      await waitFor(() => expect(input.closest('dialog')).toHaveProperty('open', true));
      expect(input).toHaveValue('');
    } finally {
      window.history.replaceState({}, '', original);
    }
  });

  it('drops ?add from the URL once the dialog has opened', async () => {
    // Otherwise a reload, or coming back to the page later, reopens the dialog.
    const original = window.location.href;
    window.history.replaceState({}, '', '/devices?add');
    try {
      const user = userEvent.setup();
      renderWithClient(<Devices />);
      await screen.findByRole('heading', { name: /enter device code/i, level: 3 });
      await waitFor(() => expect(window.location.search).toBe(''));
      expect(window.location.pathname).toBe('/devices');

      // Closing leaves it off too, by whichever route the dialog is dismissed.
      await user.keyboard('{Escape}');
      expect(window.location.search).toBe('');
    } finally {
      window.history.replaceState({}, '', original);
    }
  });

  it('shows the approved device once it appears on a later fetch', async () => {
    // API-045 creates the device row on the device's next poll, not on approve,
    // so the first refresh after approval legitimately misses it.
    const user = userEvent.setup();
    let approved = false;
    let fetchesAfterApproval = 0;
    const newDevice = { ...TEST_DEVICES[0], id: 'device-new', name: 'Paired Desktop' };
    server.use(
      http.post(`${BASE}/device-code/lookup`, () =>
        HttpResponse.json({ name: 'Paired Desktop', platform: 'linux', expires_at: Date.now() }),
      ),
      http.post(`${BASE}/device-code/approve`, () => {
        approved = true;
        return HttpResponse.json({ name: 'Paired Desktop', platform: 'linux' });
      }),
      http.get(`${BASE}/device`, () => {
        if (approved && ++fetchesAfterApproval > 1) {
          return HttpResponse.json([...TEST_DEVICES, newDevice]);
        }
        return HttpResponse.json(TEST_DEVICES);
      }),
    );

    renderWithClient(<Devices />);
    await user.click(await screen.findByRole('button', { name: /add device/i }));
    const dialog = screen.getByRole('dialog');
    await user.type(within(dialog).getByLabelText(/device code/i), 'K7RM3X');
    await user.click(within(dialog).getByRole('button', { name: /continue/i }));
    await user.click(await within(dialog).findByRole('button', { name: /^add$/i }));

    // Scoped to the device cards on purpose: the dialog is closed but still in
    // the DOM, and its confirmation summary names the same device.
    await waitFor(
      () => {
        const cardNames = Array.from(document.querySelectorAll('.vi-card__name')).map(
          (el) => el.textContent,
        );
        expect(cardNames).toContain('Paired Desktop');
      },
      { timeout: 5000 },
    );
  });

  it('looks up a code, confirms the device, then approves it', async () => {
    let lookupBody: unknown;
    let approveBody: unknown;
    server.use(
      http.post(`${BASE}/device-code/lookup`, async ({ request }) => {
        lookupBody = await request.json();
        return HttpResponse.json({
          name: 'Work Laptop',
          platform: 'linux',
          expires_at: Date.now() + 600_000,
        });
      }),
      http.post(`${BASE}/device-code/approve`, async ({ request }) => {
        approveBody = await request.json();
        return HttpResponse.json({ name: 'Work Laptop', platform: 'linux' });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Devices />);

    await user.click(await screen.findByRole('button', { name: /add device/i }));
    await user.type(screen.getByLabelText(/device code/i), 'k7r m3x');
    await user.click(screen.getByRole('button', { name: /continue/i }));

    // Step two names the device, so the user can see what they are adding.
    // Scoped to the dialog: the device list behind it also lists platforms.
    const dialog = screen.getByRole('dialog');
    await waitFor(() => {
      expect(within(dialog).getByText('Work Laptop')).toBeInTheDocument();
      expect(within(dialog).getByText('linux')).toBeInTheDocument();
    });
    expect(lookupBody).toEqual({ user_code: 'K7RM3X' });

    await user.click(screen.getByRole('button', { name: /^add$/i }));

    await waitFor(() => {
      expect(approveBody).toEqual({ user_code: 'K7RM3X' });
    });
  });

  it('formats the code as XXX-XXX however it is typed or pasted', async () => {
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    await user.click(await screen.findByRole('button', { name: /add device/i }));
    const input = screen.getByLabelText(/device code/i) as HTMLInputElement;

    // Typed straight through, no dash.
    await user.type(input, 'K7RM3X');
    expect(input.value).toBe('K7R-M3X');

    // Pasted with a dash already in it, and past the six-character limit.
    await user.clear(input);
    await user.paste('k7r-m3xzz');
    expect(input.value).toBe('K7R-M3X');

    // Backspacing off the end drops the dash again rather than stranding it.
    await user.clear(input);
    await user.type(input, 'K7RM');
    expect(input.value).toBe('K7R-M');
    await user.type(input, '{Backspace}');
    expect(input.value).toBe('K7R');
  });

  it('keeps the caret where the user was typing', async () => {
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    await user.click(await screen.findByRole('button', { name: /add device/i }));
    const input = screen.getByLabelText(/device code/i) as HTMLInputElement;

    // `type` clicks first, which parks the caret at the end; `keyboard` types
    // wherever the caret already is, which is the point of these assertions.
    await user.type(input, 'K7RM');
    expect(input.selectionStart).toBe(5);

    // Editing mid-string leaves the caret after the character just typed
    // rather than dumping it at the end of the box.
    await user.keyboard('3X');
    input.setSelectionRange(1, 1);
    await user.keyboard('9');
    expect(input.value).toBe('K97-RM3');
    expect(input.selectionStart).toBe(2);

    // A rejected character moves nothing at all.
    await user.keyboard('-');
    expect(input.value).toBe('K97-RM3');
    expect(input.selectionStart).toBe(2);

    // Backspacing the dash takes the code character before it, and the caret
    // follows that deletion instead of jumping to the end.
    input.setSelectionRange(4, 4);
    await user.keyboard('{Backspace}');
    expect(input.value).toBe('K9R-M3');
    expect(input.selectionStart).toBe(2);
  });

  it('shows the error the server returns for an invalid code', async () => {
    server.use(
      http.post(`${BASE}/device-code/lookup`, () =>
        HttpResponse.json(
          { error: 'That code is not valid. It may have expired.' },
          { status: 404 },
        ),
      ),
    );

    const user = userEvent.setup();
    renderWithClient(<Devices />);

    await user.click(await screen.findByRole('button', { name: /add device/i }));
    await user.type(screen.getByLabelText(/device code/i), 'zzzzzz');
    await user.click(screen.getByRole('button', { name: /continue/i }));

    await waitFor(() => {
      expect(screen.getByText(/that code is not valid/i)).toBeInTheDocument();
    });
    // The confirmation step must not appear for a code the server rejected.
    expect(screen.queryByRole('button', { name: /^add$/i })).not.toBeInTheDocument();
  });
});

describe('Devices — Device rename', () => {
  it('calls PATCH /device/:id on rename submit', async () => {
    let patchBody: unknown;
    let patchedId: string | undefined;
    server.use(
      http.patch(`${BASE}/device/:id`, async ({ request, params }) => {
        patchedId = params.id as string;
        patchBody = await request.json();
        return HttpResponse.json({ ...TEST_DEVICES[0], name: 'Renamed' });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Devices />);

    const editBtns = await screen.findAllByRole('button', { name: /^edit$/i });
    await user.click(editBtns[0]);

    const nameInput = screen.getByDisplayValue('My Laptop');
    await user.clear(nameInput);
    await user.type(nameInput, 'Renamed');
    await user.click(screen.getByRole('button', { name: /save/i }));

    await waitFor(() => {
      expect(patchedId).toBe('device-1');
      expect((patchBody as { name: string }).name).toBe('Renamed');
    });
  });
});

describe('Devices — Device delete', () => {
  it('calls DELETE /device/:id after confirmation', async () => {
    let deletedId: string | undefined;
    server.use(
      http.delete(`${BASE}/device/:id`, ({ params }) => {
        deletedId = params.id as string;
        return new HttpResponse(null, { status: 204 });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Devices />);

    const editBtns = await screen.findAllByRole('button', { name: /^edit$/i });
    await user.click(editBtns[0]);

    const deleteDeviceBtns = screen.getAllByRole('button', { name: /delete device/i });
    await user.click(deleteDeviceBtns[0]);

    const confirmBtns = screen.getAllByRole('button', { name: /delete device/i });
    await user.click(confirmBtns[confirmBtns.length - 1]);

    await waitFor(() => {
      expect(deletedId).toBe('device-1');
    });
  });
});
