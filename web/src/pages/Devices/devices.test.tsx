import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { describe, expect, it } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_DEVICES } from '../../mocks/fixtures';
import { renderWithClient } from '../../test-utils';
import { Devices } from './index';

describe('Devices — device list', () => {
  it('renders device names once loaded', async () => {
    renderWithClient(<Devices />);
    await waitFor(() => {
      expect(screen.getByText('My Laptop')).toBeInTheDocument();
      expect(screen.getByText('My Phone')).toBeInTheDocument();
    });
  });

  it('shows "My devices" section heading', async () => {
    renderWithClient(<Devices />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /my devices/i })).toBeInTheDocument();
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
});

describe('Devices — Device rename', () => {
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
      http.delete('http://localhost:8787/device/:id', ({ params }) => {
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
