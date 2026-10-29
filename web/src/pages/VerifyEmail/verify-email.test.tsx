import { afterEach, describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/preact';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { server } from '../../mocks/server';
import { VerifyEmail } from './index';

const API = `http://localhost:8787/${CURRENT_API_VERSION}`;

afterEach(() => {
  window.history.replaceState({}, '', '/');
});

function openLink(search: string) {
  window.history.replaceState({}, '', `/verify-email${search}`);
  return render(<VerifyEmail />);
}

describe('Email verification (WEB-033)', () => {
  it('shows "Verifying…" until the server answers (WEB-033)', async () => {
    let answer: () => void = () => {};
    const gate = new Promise<void>((resolve) => (answer = resolve));
    server.use(
      http.post(`${API}/email-verification/validate`, async () => {
        await gate;
        return HttpResponse.json({ ok: true });
      }),
    );
    openLink('?token=abc');

    expect(screen.getByText('Verifying…')).toBeInTheDocument();
    expect(screen.queryByText('Email updated')).not.toBeInTheDocument();
    expect(
      screen.queryByText('This verification link is invalid or has expired.'),
    ).not.toBeInTheDocument();

    answer();
    expect(await screen.findByText('Email updated')).toBeInTheDocument();
    expect(screen.queryByText('Verifying…')).not.toBeInTheDocument();
  });

  it('sends the token from the link and then says the email was updated, with a button to Settings (WEB-033)', async () => {
    let body: unknown;
    server.use(
      http.post(`${API}/email-verification/validate`, async ({ request }) => {
        body = await request.json();
        return HttpResponse.json({ ok: true });
      }),
    );
    openLink('?token=tok-123');

    expect(await screen.findByText('Email updated')).toBeInTheDocument();
    expect(body).toEqual({ token: 'tok-123' });
    expect(screen.getByText('Your email address has been successfully updated.')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Go to settings' })).toBeEnabled();
  });

  it.each([
    ['rejects the token', () => HttpResponse.json({ error: 'Invalid token' }, { status: 400 })],
    ['cannot be reached', () => HttpResponse.error()],
  ])('says the link is invalid or expired when the server %s (WEB-033)', async (_name, respond) => {
    server.use(http.post(`${API}/email-verification/validate`, respond));
    openLink('?token=bad');

    expect(
      await screen.findByText('This verification link is invalid or has expired.'),
    ).toBeInTheDocument();
    expect(screen.queryByText('Email updated')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Go to settings' })).not.toBeInTheDocument();
  });

  it.each(['', '?token='])(
    'shows the error without asking the server for a link with no token (%j) (WEB-033)',
    async (search) => {
      let asked = false;
      server.use(
        http.post(`${API}/email-verification/validate`, () => {
          asked = true;
          return HttpResponse.json({ ok: true });
        }),
      );
      openLink(search);

      expect(
        await screen.findByText('This verification link is invalid or has expired.'),
      ).toBeInTheDocument();
      expect(asked).toBe(false);
    },
  );
});
