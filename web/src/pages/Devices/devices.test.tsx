import { screen, waitFor, within } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { useState } from 'preact/hooks';
import { describe, expect, it } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_DEVICES, TEST_USER } from '../../mocks/fixtures';
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

describe('Devices — DNS filter check (WEB-072, WEB-035)', () => {
  function withAccountDomain(domain: string | null) {
    server.use(
      http.get(`${BASE}/user`, () =>
        HttpResponse.json({ ...TEST_USER, dns_filter_domain: domain }),
      ),
    );
  }

  async function openEditor(user: ReturnType<typeof userEvent.setup>) {
    const editBtns = await screen.findAllByRole('button', { name: /^edit$/i });
    await user.click(editBtns[0]);
    return screen.getAllByRole('switch', { name: 'Check the DNS filter on this device' })[0];
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

  it('shows "On" or "Off" on the card when the account has a domain', async () => {
    withAccountDomain('block.example.net');
    server.use(
      http.get(`${BASE}/device`, () =>
        HttpResponse.json([{ ...TEST_DEVICES[0], dns_filter_disabled: true }, TEST_DEVICES[1]]),
      ),
    );
    renderWithClient(<Devices />);
    expect(await screen.findByText('On')).toBeInTheDocument();
    expect(screen.getByText('Off')).toBeInTheDocument();
  });

  it('shows "Not set up" on the card when the account has no domain, and "Off" still wins', async () => {
    withAccountDomain(null);
    server.use(
      http.get(`${BASE}/device`, () =>
        HttpResponse.json([{ ...TEST_DEVICES[0], dns_filter_disabled: true }, TEST_DEVICES[1]]),
      ),
    );
    renderWithClient(<Devices />);
    expect(await screen.findByText('Not set up')).toBeInTheDocument();
    expect(screen.getByText('Off')).toBeInTheDocument();
  });

  it('sends dns_filter_disabled when the owner turns the check off, naming the domain in the hint', async () => {
    withAccountDomain('block.example.net');
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    const dnsSwitch = await openEditor(user);
    expect(dnsSwitch).toBeChecked();
    expect(
      await screen.findAllByText(/your DNS check domain, block\.example\.net, stays blocked/i),
    ).not.toHaveLength(0);
    expect(screen.getAllByText(/partners are notified when you turn it off/i)).not.toHaveLength(0);
    await user.click(dnsSwitch);
    await user.click(screen.getByRole('button', { name: /save/i }));
    await user.click(await screen.findByRole('button', { name: 'Turn off' }));

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(bodies[0]).toEqual({ name: 'My Laptop', dns_filter_disabled: true });
  });

  const offTitle = 'Turn off the DNS check on this device?';
  const confirmHeading = () => screen.queryByRole('heading', { name: offTitle });
  // The browser fires a cancelable `cancel` event on Escape, then closes the dialog.
  const pressEscape = () => {
    const dialog = screen.getByRole('heading', { name: offTitle }).closest('dialog')!;
    if (dialog.dispatchEvent(new Event('cancel', { cancelable: true }))) dialog.close();
  };

  it('asks only on save, not when the switch is flipped, and defaults to "Keep it on"', async () => {
    withAccountDomain('block.example.net');
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    const dnsSwitch = await openEditor(user);
    await user.click(dnsSwitch);
    expect(confirmHeading()).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: /save/i }));

    expect(await screen.findByRole('heading', { name: offTitle })).toBeInTheDocument();
    expect(
      within(screen.getByRole('heading', { name: offTitle }).closest('dialog')!).getByText(
        /if you turn it off for this device, your partners are notified/i,
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Keep it on' })).toHaveAttribute('autofocus');
    expect(bodies).toHaveLength(0);
  });

  it('keeps the edit dialog open with the switch back on, and sends nothing, on "Keep it on"', async () => {
    withAccountDomain('block.example.net');
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    const dnsSwitch = await openEditor(user);
    await user.click(dnsSwitch);
    await user.click(screen.getByRole('button', { name: /save/i }));
    await user.click(await screen.findByRole('button', { name: 'Keep it on' }));

    expect(confirmHeading()).not.toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Edit device' })).toBeInTheDocument();
    expect(dnsSwitch).toBeChecked();
    expect(bodies).toHaveLength(0);
  });

  it('treats Escape on the question like "Keep it on"', async () => {
    withAccountDomain('block.example.net');
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    const dnsSwitch = await openEditor(user);
    await user.click(dnsSwitch);
    await user.click(screen.getByRole('button', { name: /save/i }));
    await screen.findByRole('heading', { name: offTitle });
    pressEscape();

    await waitFor(() => expect(confirmHeading()).not.toBeInTheDocument());
    expect(screen.getByRole('heading', { name: 'Edit device' })).toBeInTheDocument();
    expect(dnsSwitch).toBeChecked();
    expect(bodies).toHaveLength(0);
  });

  it('sends the rename and the switch together on "Turn off", and keeps the rename when cancelled', async () => {
    withAccountDomain('block.example.net');
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    const dnsSwitch = await openEditor(user);
    const nameInput = screen.getByDisplayValue('My Laptop');
    await user.clear(nameInput);
    await user.type(nameInput, 'Renamed');
    await user.click(dnsSwitch);
    await user.click(screen.getByRole('button', { name: /save/i }));
    await user.click(await screen.findByRole('button', { name: 'Keep it on' }));
    expect(bodies).toHaveLength(0);
    expect(screen.getByDisplayValue('Renamed')).toBeInTheDocument();

    await user.click(dnsSwitch);
    await user.click(screen.getByRole('button', { name: /save/i }));
    await user.click(await screen.findByRole('button', { name: 'Turn off' }));

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(bodies[0]).toEqual({ name: 'Renamed', dns_filter_disabled: true });
  });

  it('does not ask when only the name changes', async () => {
    withAccountDomain('block.example.net');
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    await openEditor(user);
    await user.type(screen.getByDisplayValue('My Laptop'), ' 2');
    await user.click(screen.getByRole('button', { name: /save/i }));

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(confirmHeading()).not.toBeInTheDocument();
  });

  it('sends dns_filter_disabled false when the owner turns the check back on', async () => {
    withAccountDomain('block.example.net');
    server.use(
      http.get(`${BASE}/device`, () =>
        HttpResponse.json([{ ...TEST_DEVICES[0], dns_filter_disabled: true }]),
      ),
    );
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    const dnsSwitch = await openEditor(user);
    expect(dnsSwitch).not.toBeChecked();
    await user.click(dnsSwitch);
    await user.click(screen.getByRole('button', { name: /save/i }));

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(confirmHeading()).not.toBeInTheDocument();
    expect(bodies[0]).toEqual({ name: 'My Laptop', dns_filter_disabled: false });
  });

  it('does not send the switch when it did not change', async () => {
    const bodies = capturePatches();
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    await openEditor(user);
    await user.click(screen.getByRole('button', { name: /save/i }));

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(bodies[0]).toEqual({ name: 'My Laptop' });
  });

  it('points to Settings when the account has no domain', async () => {
    withAccountDomain(null);
    const user = userEvent.setup();
    renderWithClient(<Devices />);

    await openEditor(user);
    const links = screen.getAllByRole('link', { name: 'Settings' });
    expect(links[0]).toHaveAttribute('href', '/settings');
    expect(screen.getAllByText(/set a DNS check domain in/i)).not.toHaveLength(0);
  });

  it("does not list or offer editing for another person's devices (partners only watch)", async () => {
    server.use(
      http.get(`${BASE}/device`, () =>
        HttpResponse.json([{ ...TEST_DEVICES[0], owner: 'someone-else', name: 'Bobs PC' }]),
      ),
    );
    renderWithClient(<Devices />);

    expect(await screen.findByText('Add your first device')).toBeInTheDocument();
    expect(screen.queryByText('Bobs PC')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^edit$/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
  });
});
