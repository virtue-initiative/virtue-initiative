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

    expect(screen.getByRole('heading', { name: /add device/i, level: 3 })).toBeInTheDocument();
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
    await user.type(screen.getByLabelText(/first three characters/i), 'k7r');
    await user.type(screen.getByLabelText(/last three characters/i), 'm3x');
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

  it('splits a full code pasted into the first box across both boxes', async () => {
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    await user.click(await screen.findByRole('button', { name: /add device/i }));
    const first = screen.getByLabelText(/first three characters/i) as HTMLInputElement;
    await user.click(first);
    await user.paste('K7R-M3X');

    expect(first.value).toBe('K7R');
    expect((screen.getByLabelText(/last three characters/i) as HTMLInputElement).value).toBe('M3X');
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
    await user.type(screen.getByLabelText(/first three characters/i), 'zzz');
    await user.type(screen.getByLabelText(/last three characters/i), 'zzz');
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
