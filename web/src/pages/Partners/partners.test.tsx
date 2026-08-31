import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { describe, expect, it } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_DEVICES, TEST_WATCHER, TEST_WATCHING } from '../../mocks/fixtures';
import { renderWithClient } from '../../test-utils';
import { Partners } from './index';

const BASE = `http://localhost:8787/${CURRENT_API_VERSION}`;

describe('Partners — sections', () => {
  it('shows "You monitor" section heading', async () => {
    renderWithClient(<Partners />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /you monitor/i })).toBeInTheDocument();
    });
  });

  it('shows "Monitor you" section heading', async () => {
    renderWithClient(<Partners />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /monitor you/i })).toBeInTheDocument();
    });
  });

  it('renders accepted watcher name', async () => {
    renderWithClient(<Partners />);
    await waitFor(() => {
      expect(screen.getByText(TEST_WATCHER.user.name!)).toBeInTheDocument();
    });
  });

  it('renders accepted watching partner name', async () => {
    renderWithClient(<Partners />);
    await waitFor(() => {
      expect(screen.getByText(TEST_WATCHING.user.name!)).toBeInTheDocument();
    });
  });

  it('shows empty messages when there are no partners', async () => {
    server.use(
      http.get(`${BASE}/partner`, () => HttpResponse.json({ watchers: [], watching: [] })),
    );
    renderWithClient(<Partners />);
    await waitFor(() => {
      expect(screen.getByText('No one can monitor you yet.')).toBeInTheDocument();
      expect(screen.getByText('You cannot monitor anyone yet.')).toBeInTheDocument();
    });
  });
});

describe('Partners — monitored device list', () => {
  it("lists a watched account's devices with their status", async () => {
    server.use(
      http.get(`${BASE}/device`, () =>
        HttpResponse.json([
          { ...TEST_DEVICES[1], owner: TEST_WATCHING.user.id },
          { ...TEST_DEVICES[0], owner: TEST_WATCHING.user.id, status: 'logged_out' },
        ]),
      ),
    );
    renderWithClient(<Partners />);

    expect(
      await screen.findByRole('heading', { name: /^devices \(last seen\)$/i }),
    ).toBeInTheDocument();

    const row = await screen.findByRole('button', { name: new RegExp(TEST_DEVICES[0].name, 'i') });
    expect(row).toHaveTextContent('Deactivated');
    // last_hash_at is seconds old in the fixtures, so it renders in the compact form.
    expect(row).toHaveTextContent('(now)');
    expect(
      await screen.findByRole('button', { name: new RegExp(TEST_DEVICES[1].name, 'i') }),
    ).toHaveTextContent('Online');
  });
});

describe('Partners — more devices dialog', () => {
  it('opens a dialog listing every device when "+N more" is clicked', async () => {
    const user = userEvent.setup();
    const many = Array.from({ length: 6 }, (_, i) => ({
      ...TEST_DEVICES[0],
      id: `many-device-${i}`,
      name: `Device ${i}`,
      owner: TEST_WATCHING.user.id,
    }));
    server.use(http.get(`${BASE}/device`, () => HttpResponse.json(many)));
    renderWithClient(<Partners />);

    const moreBtn = await screen.findByRole('button', { name: /\+2 more devices/i });
    // The card itself lists only the first four.
    expect(screen.queryByRole('button', { name: /Device 5/ })).not.toBeInTheDocument();

    await user.click(moreBtn);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Device 5/ })).toBeInTheDocument();
    });
    expect(screen.getAllByRole('button', { name: /^close$/i }).length).toBeGreaterThan(0);
  });
});

describe('Partners — Invite partner dialog', () => {
  it('opens the invite dialog with an email input', async () => {
    const user = userEvent.setup();
    renderWithClient(<Partners />);

    const inviteBtn = await screen.findByRole('button', { name: /invite partner/i });
    await user.click(inviteBtn);

    expect(screen.getByPlaceholderText('partner@example.com')).toBeInTheDocument();
  });

  it('calls POST /partner when invite is submitted', async () => {
    let inviteBody: unknown;
    server.use(
      http.post(`${BASE}/partner`, async ({ request }) => {
        inviteBody = await request.json();
        return HttpResponse.json({ id: 'new-1', invite_token: 'tok' });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Partners />);

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
