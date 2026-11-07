import { afterEach, describe, expect, it } from 'vitest';
import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { server } from '../../mocks/server';
import { TEST_WATCHING } from '../../mocks/fixtures';
import { renderWithClient } from '../../test-utils';
import { InviteAccept } from './index';

const API = `http://localhost:8787/${CURRENT_API_VERSION}`;

afterEach(() => {
  window.history.replaceState({}, '', '/');
});

function openLink(search = '?partner_token=tok-1') {
  window.history.replaceState({}, '', `/invite-accept${search}`);
  return renderWithClient(<InviteAccept />);
}

/** The signed-in user has no accepted partnership yet. */
function firstPartnership(watching: unknown[] = []) {
  server.use(http.get(`${API}/partner`, () => HttpResponse.json({ watchers: [], watching })));
}

const owner = { id: 'owner-1', name: 'Dana Owner', email: 'dana@example.com' };
function inviteFrom(who: { id: string; name?: string | null; email: string }) {
  server.use(
    http.post(`${API}/partner/validate`, () =>
      HttpResponse.json({ ok: true, partnership_id: 'p-1', owner: who }),
    ),
  );
}

describe('Invite acceptance (WEB-053)', () => {
  it('shows "Processing invite…" while the invite is being accepted (WEB-053)', async () => {
    let finish: () => void = () => {};
    const gate = new Promise<void>((resolve) => (finish = resolve));
    server.use(
      http.post(`${API}/partner/accept`, async () => {
        await gate;
        return HttpResponse.json({ id: 'p-1' });
      }),
    );
    openLink();

    expect(await screen.findByText('Processing invite…')).toBeInTheDocument();
    expect(screen.queryByText('Invite error')).not.toBeInTheDocument();
    finish();
    expect(await screen.findByText(/accepted\./)).toBeInTheDocument();
    expect(screen.queryByText('Processing invite…')).not.toBeInTheDocument();
  });

  it('validates the token, then accepts the invite with it (WEB-053)', async () => {
    const calls: string[] = [];
    server.use(
      http.post(`${API}/partner/validate`, async ({ request }) => {
        calls.push(`validate ${JSON.stringify(await request.json())}`);
        return HttpResponse.json({ ok: true, partnership_id: 'p-1', owner });
      }),
      http.post(`${API}/partner/accept`, async ({ request }) => {
        calls.push(`accept ${JSON.stringify(await request.json())}`);
        return HttpResponse.json({ id: 'p-1' });
      }),
    );
    openLink('?partner_token=tok-xyz');

    await screen.findByText(/accepted\./);
    expect(calls).toEqual(['validate {"token":"tok-xyz"}', 'accept {"token":"tok-xyz"}']);
  });

  it('says whose invite was accepted, by name (WEB-053)', async () => {
    inviteFrom(owner);
    openLink();
    expect(
      await screen.findByText(
        (_, el) =>
          el?.tagName === 'P' &&
          el.textContent ===
            'Invite from Dana Owner accepted. You can now view their monitoring data.',
      ),
    ).toBeInTheDocument();
  });

  it('falls back to the email when the inviter has no name (WEB-053)', async () => {
    inviteFrom({ id: 'owner-2', name: null, email: 'noname@example.com' });
    openLink();
    expect(
      await screen.findByText(
        (_, el) =>
          el?.tagName === 'P' &&
          el.textContent ===
            'Invite from noname@example.com accepted. You can now view their monitoring data.',
      ),
    ).toBeInTheDocument();
  });

  it('offers "Back to dashboard" instead of the email question when the user already watches someone (WEB-053)', async () => {
    const user = userEvent.setup();
    openLink(); // the default partner list already has an accepted partner

    const back = await screen.findByRole('button', { name: 'Back to dashboard' });
    expect(screen.queryByRole('button', { name: 'Done' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Weekly' })).not.toBeInTheDocument();
    await user.click(back);
    expect(window.location.pathname).toBe('/');
  });

  describe('the first accepted partnership', () => {
    it('asks how often to send summary emails, defaulting to Weekly (WEB-053)', async () => {
      firstPartnership();
      openLink();

      expect(await screen.findByRole('button', { name: 'Done' })).toBeEnabled();
      expect(screen.getByRole('button', { name: 'Weekly' })).toHaveClass('is-active');
      expect(screen.getByRole('button', { name: 'Daily' })).not.toHaveClass('is-active');
      expect(screen.queryByRole('button', { name: 'Back to dashboard' })).not.toBeInTheDocument();
    });

    it("doesn't count a pending invite as an accepted partnership (WEB-053)", async () => {
      firstPartnership([{ ...TEST_WATCHING, status: 'pending' }]);
      openLink();
      expect(await screen.findByRole('button', { name: 'Done' })).toBeInTheDocument();
    });

    it.each([
      ['the default', null, 'weekly'],
      ['Daily', 'Daily', 'daily'],
      ['Weekly picked again', 'Weekly', 'weekly'],
    ])(
      'Done saves %s as the email frequency and goes to the dashboard (WEB-053)',
      async (_name, click, saved) => {
        let patch: unknown;
        server.use(
          http.patch(`${API}/user`, async ({ request }) => {
            patch = await request.json();
            return HttpResponse.json({ email_verification_required: false });
          }),
        );
        firstPartnership();
        const user = userEvent.setup();
        openLink();
        if (click) await user.click(await screen.findByRole('button', { name: click }));
        await user.click(await screen.findByRole('button', { name: 'Done' }));

        await waitFor(() => expect(window.location.pathname).toBe('/'));
        expect(patch).toEqual({ settings: { email_frequency: saved } });
      },
    );

    it('shows an error and stays on the page when saving the choice fails (WEB-053)', async () => {
      server.use(
        http.patch(`${API}/user`, () =>
          HttpResponse.json({ error: 'Could not save settings' }, { status: 500 }),
        ),
      );
      firstPartnership();
      const user = userEvent.setup();
      openLink();
      await user.click(await screen.findByRole('button', { name: 'Done' }));

      expect(
        await screen.findByText(
          'Something went wrong, please try again. If this continues, contact help@virtueinitiative.org.',
        ),
      ).toBeInTheDocument();
      expect(window.location.pathname).toBe('/invite-accept');
      expect(screen.getByRole('button', { name: 'Done' })).toBeEnabled();
    });
  });

  it.each([
    ['an expired invite', 410, 'This invite has expired.'],
    ['an invite for another email', 403, 'This invite was sent to a different email address.'],
  ])(
    'shows "Invite error" with the server\'s message for %s (WEB-053)',
    async (_name, status, error) => {
      server.use(
        http.post(`${API}/partner/accept`, () => HttpResponse.json({ error }, { status })),
      );
      const user = userEvent.setup();
      openLink();

      expect(await screen.findByText('Invite error')).toBeInTheDocument();
      expect(screen.getByText(error)).toBeInTheDocument();
      expect(screen.queryByText(/accepted\./)).not.toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Back to dashboard' }));
      expect(window.location.pathname).toBe('/');
    },
  );

  it.each(['', '?partner_token=', '?token=wrong-name'])(
    'shows "Missing invite token." for a link without one (%j), and sends nothing (WEB-053)',
    async (search) => {
      let sent = false;
      server.use(
        http.post(`${API}/partner/accept`, () => {
          sent = true;
          return HttpResponse.json({ id: 'p-1' });
        }),
      );
      openLink(search);

      expect(await screen.findByText('Invite error')).toBeInTheDocument();
      expect(screen.getByText('Missing invite token.')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Back to dashboard' })).toBeInTheDocument();
      expect(sent).toBe(false);
    },
  );
});
