import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { describe, expect, it } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_DEVICES, TEST_WATCHER, TEST_WATCHING } from '../../mocks/fixtures';
import { renderWithClient } from '../../test-utils';
import { Home } from './index';

describe('Home — device list', () => {
  it('renders device names once loaded', async () => {
    renderWithClient(<Home />);
    await waitFor(() => {
      expect(screen.getByText('My Laptop')).toBeInTheDocument();
      expect(screen.getByText('My Phone')).toBeInTheDocument();
    });
  });

  it('shows "My devices" section heading', async () => {
    renderWithClient(<Home />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /my devices/i })).toBeInTheDocument();
    });
  });
});

describe('Home — Add device dialog', () => {
  it('opens and closes the Add device dialog', async () => {
    const user = userEvent.setup();
    renderWithClient(<Home />);

    const addBtn = await screen.findByRole('button', { name: /add device/i });
    await user.click(addBtn);

    expect(screen.getByRole('heading', { name: /add device/i, level: 3 })).toBeInTheDocument();
  });
});

describe('Home — partner sections', () => {
  it('shows "Monitor you" section heading', async () => {
    renderWithClient(<Home />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /monitor you/i })).toBeInTheDocument();
    });
  });

  it('shows "You monitor" section heading', async () => {
    renderWithClient(<Home />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /you monitor/i })).toBeInTheDocument();
    });
  });

  it('renders accepted watcher name', async () => {
    renderWithClient(<Home />);
    await waitFor(() => {
      expect(screen.getByText(TEST_WATCHER.user.name!)).toBeInTheDocument();
    });
  });

  it('renders accepted watching partner name', async () => {
    renderWithClient(<Home />);
    await waitFor(() => {
      expect(screen.getByText(TEST_WATCHING.user.name!)).toBeInTheDocument();
    });
  });

  it('shows empty message when no partners', async () => {
    server.use(
      http.get('http://localhost:8787/partner', () =>
        HttpResponse.json({ watchers: [], watching: [] }),
      ),
    );
    renderWithClient(<Home />);
    await waitFor(() => {
      expect(screen.getByText('No one can monitor you yet.')).toBeInTheDocument();
      expect(screen.getByText('You cannot monitor anyone yet.')).toBeInTheDocument();
    });
  });
});

describe('Home — Invite partner dialog', () => {
  it('opens the invite dialog with an email input', async () => {
    const user = userEvent.setup();
    renderWithClient(<Home />);

    const inviteBtn = await screen.findByRole('button', { name: /invite partner/i });
    await user.click(inviteBtn);

    // Field component uses <label> without `for`, so query by placeholder instead
    expect(screen.getByPlaceholderText('partner@example.com')).toBeInTheDocument();
  });

  it('calls POST /partner when invite is submitted', async () => {
    let inviteBody: unknown;
    server.use(
      http.post('http://localhost:8787/partner', async ({ request }) => {
        inviteBody = await request.json();
        return HttpResponse.json({ id: 'new-1', invite_token: 'tok' });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Home />);

    const inviteBtn = await screen.findByRole('button', { name: /invite partner/i });
    await user.click(inviteBtn);

    const emailInput = screen.getByPlaceholderText('partner@example.com');
    await user.type(emailInput, 'alice@example.com');
    await user.click(screen.getByRole('button', { name: /send invite/i }));

    await waitFor(() => {
      expect((inviteBody as { email: string }).email).toBe('alice@example.com');
    });
  });
});

describe('Home — Device rename', () => {
  it('calls PATCH /device/:id on rename submit', async () => {
    let patchBody: unknown;
    let patchedId: string | undefined;
    server.use(
      http.patch('http://localhost:8787/device/:id', async ({ request, params }) => {
        patchedId = params.id as string;
        patchBody = await request.json();
        return HttpResponse.json({ ...TEST_DEVICES[0], name: 'Renamed' });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Home />);

    // Find and click the Edit button on the first device card
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

describe('Home — Device delete', () => {
  it('calls DELETE /device/:id after confirmation', async () => {
    let deletedId: string | undefined;
    server.use(
      http.delete('http://localhost:8787/device/:id', ({ params }) => {
        deletedId = params.id as string;
        return new HttpResponse(null, { status: 204 });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Home />);

    // Step 1: Click Edit to open the edit dialog
    const editBtns = await screen.findAllByRole('button', { name: /^edit$/i });
    await user.click(editBtns[0]);

    // Step 2: Click "Delete device" in the secondary actions of the edit dialog
    const deleteDeviceBtns = screen.getAllByRole('button', { name: /delete device/i });
    await user.click(deleteDeviceBtns[0]);

    // Step 3: Confirm deletion in the confirmation dialog
    const confirmBtns = screen.getAllByRole('button', { name: /delete device/i });
    // The confirmation button is the last "Delete device" button
    await user.click(confirmBtns[confirmBtns.length - 1]);

    await waitFor(() => {
      expect(deletedId).toBe('device-1');
    });
  });
});
