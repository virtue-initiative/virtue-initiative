import { screen, waitFor, within } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { useState } from 'preact/hooks';
import { describe, expect, it } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_DEVICES } from '../../mocks/fixtures';
import { renderWithClient } from '../../test-utils';
import { Devices } from './index';

const BASE = `http://localhost:8787/${CURRENT_API_VERSION}`;

describe('Devices — device list (WEB-034)', () => {
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

  it('shows a loading indicator instead of the empty state while the fetch is in flight (WEB-009)', async () => {
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
    expect(screen.queryByText('Add your first device')).not.toBeInTheDocument();

    await waitFor(() => expect(resolveDevices).toBeDefined());
    resolveDevices?.();

    await waitFor(() => {
      expect(screen.getByText('My Laptop')).toBeInTheDocument();
    });
  });

  it('shows setup instructions once loaded with an empty list', async () => {
    server.use(http.get(`${BASE}/device`, () => HttpResponse.json([])));

    renderWithClient(<Devices />);

    const heading = await screen.findByRole('heading', {
      name: 'Add your first device',
      level: 2,
    });
    const emptyState = within(heading.closest('section')!);
    expect(emptyState.getByText('Download the app.')).toBeInTheDocument();
    expect(emptyState.getByRole('link', { name: 'Download the app' })).toHaveAttribute(
      'href',
      expect.stringMatching(/\/download$/),
    );
  });

  it('does not show setup instructions when devices exist', async () => {
    renderWithClient(<Devices />);
    await screen.findByText('My Laptop');
    expect(screen.queryByText('Add your first device')).not.toBeInTheDocument();
  });
});

describe('Devices — refresh on open (WEB-012, WEB-034)', () => {
  it('shows the cached devices on reopen and then refetches them', async () => {
    const user = userEvent.setup();
    function Harness() {
      const [open, setOpen] = useState(true);
      return (
        <>
          <button type="button" onClick={() => setOpen((o) => !o)}>
            toggle
          </button>
          {open && <Devices />}
        </>
      );
    }

    renderWithClient(<Harness />);
    await screen.findByText('My Laptop');
    await user.click(screen.getByRole('button', { name: 'toggle' }));
    expect(screen.queryByText('My Laptop')).not.toBeInTheDocument();

    let resolveDevices: (() => void) | undefined;
    server.use(
      http.get(
        `${BASE}/device`,
        () =>
          new Promise((resolve) => {
            resolveDevices = () =>
              resolve(
                HttpResponse.json(
                  TEST_DEVICES.map((d) =>
                    d.name === 'My Laptop' ? { ...d, name: 'Work Laptop' } : d,
                  ),
                ),
              );
          }),
      ),
    );

    await user.click(screen.getByRole('button', { name: 'toggle' }));
    // Cached copy renders immediately while the refetch is in flight.
    expect(screen.getByText('My Laptop')).toBeInTheDocument();
    expect(screen.queryByText(/loading/i)).not.toBeInTheDocument();

    await waitFor(() => expect(resolveDevices).toBeDefined());
    resolveDevices?.();
    expect(await screen.findByText('Work Laptop')).toBeInTheDocument();
    expect(screen.queryByText('My Laptop')).not.toBeInTheDocument();
  });
});

describe('Devices — Add device dialog (WEB-036)', () => {
  it('opens the Add device dialog', async () => {
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    const addBtn = await screen.findByRole('button', { name: /add device/i });
    await user.click(addBtn);

    expect(screen.getByRole('heading', { name: /add device/i, level: 3 })).toBeInTheDocument();
  });
});

describe('Devices — Device rename (WEB-037)', () => {
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

describe('Devices — Device delete (WEB-037, WEB-020)', () => {
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

describe('Devices — DNS check domain (WEB-072, WEB-035)', () => {
  async function openEditor(user: ReturnType<typeof userEvent.setup>) {
    const editBtns = await screen.findAllByRole('button', { name: /^edit$/i });
    await user.click(editBtns[0]);
    return screen.getAllByLabelText('DNS check domain')[0];
  }

  function capturePatches() {
    const bodies: Record<string, unknown>[] = [];
    server.use(
      http.patch(`${BASE}/device/:id`, async ({ request }) => {
        bodies.push((await request.json()) as Record<string, unknown>);
        return new HttpResponse(null, { status: 204 });
      }),
    );
    return bodies;
  }

  it('shows the current domain on the card, or "Not set"', async () => {
    server.use(
      http.get(`${BASE}/device`, () =>
        HttpResponse.json([
          { ...TEST_DEVICES[0], dns_filter_domain: 'block.example.net' },
          TEST_DEVICES[1],
        ]),
      ),
    );
    renderWithClient(<Devices />);
    expect(await screen.findByText('block.example.net')).toBeInTheDocument();
    expect(screen.getByText('Not set')).toBeInTheDocument();
  });

  it('sends the domain lowercased and trimmed when the owner sets it', async () => {
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    const input = await openEditor(user);
    expect(
      screen.getAllByText(/add this domain to your dns filter's block list/i),
    ).not.toHaveLength(0);
    await user.type(input, '  Block.Example.NET ');
    await user.click(screen.getByRole('button', { name: /save/i }));

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(bodies[0]).toEqual({ name: 'My Laptop', dns_filter_domain: 'block.example.net' });
  });

  it('shows the validation message and sends nothing for an invalid domain', async () => {
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    const input = await openEditor(user);
    await user.type(input, 'https://example.net/path');
    await user.click(screen.getByRole('button', { name: /save/i }));

    expect(await screen.findByText('Enter a domain such as example.net')).toBeInTheDocument();
    expect(input).toHaveAttribute('aria-invalid', 'true');
    expect(bodies).toHaveLength(0);
  });

  it('sends null when the owner clears the domain', async () => {
    server.use(
      http.get(`${BASE}/device`, () =>
        HttpResponse.json([{ ...TEST_DEVICES[0], dns_filter_domain: 'block.example.net' }]),
      ),
    );
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    await screen.findByText('block.example.net');
    const input = await openEditor(user);
    await user.clear(input);
    await user.click(screen.getByRole('button', { name: /save/i }));

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(bodies[0].dns_filter_domain).toBeNull();
  });

  it("does not list or offer editing for another person's devices (partners only watch)", async () => {
    server.use(
      http.get(`${BASE}/device`, () =>
        HttpResponse.json([
          {
            ...TEST_DEVICES[0],
            owner: 'someone-else',
            name: 'Bobs PC',
            dns_filter_domain: 'x.example.net',
          },
        ]),
      ),
    );
    renderWithClient(<Devices />);

    expect(await screen.findByText('Add your first device')).toBeInTheDocument();
    expect(screen.queryByText('Bobs PC')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^edit$/i })).not.toBeInTheDocument();
  });
});
