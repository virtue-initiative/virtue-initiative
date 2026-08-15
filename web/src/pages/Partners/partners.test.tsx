import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { describe, expect, it } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_WATCHER, TEST_WATCHING } from '../../mocks/fixtures';
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
