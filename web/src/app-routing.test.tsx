// The whole app (src/index.tsx) as a user meets it: which page each URL opens, what a
// signed-out visitor can reach, how login returns them where they were going, logging
// out, and the one-time messages that survive a reload.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { server } from './mocks/server';
import { TEST_USER, TEST_WATCHING } from './mocks/fixtures';
import { makeFakeSession } from './test-utils';
import { Session } from './utils/api/session';
import { App } from './index';

const API = `http://localhost:8787/${CURRENT_API_VERSION}`;
const WRAPPING_KEY_STORAGE = 'virtue_wrapping_key';
const MESSAGE_KEY = 'virtue_global_link_message';

// Importing the entry point mounts an App into document.body for real; a test renders its own.
vi.mock('preact-iso', async (importOriginal) => ({
  ...(await importOriginal<typeof import('preact-iso')>()),
  hydrate: () => {},
}));

const clearCache = vi.hoisted(() => vi.fn());

vi.mock('./utils/cache/client', () => ({
  cacheClient: {
    setSession: () => {},
    clearCache,
    resetCache: async () => {},
    refetch: () => {},
    cacheQuery: (_query: unknown, cb: (update: unknown) => void) => {
      Promise.resolve().then(() =>
        cb({ logs: [], replace: true, done: true, processed: 0, total: 0 }),
      );
    },
    getDeviceLastUploads: async () => ({}),
    getDeviceBatchEndTimes: async () => [],

    getEventImage: async () => null,
    deleteDeviceData: async () => {},
  },
}));

/** What a browser holds after a login: the key that unlocks the user's private key. */
function saveSession() {
  const raw = crypto.getRandomValues(new Uint8Array(32));
  localStorage.setItem(WRAPPING_KEY_STORAGE, btoa(String.fromCharCode(...raw)));
}

function visit(url: string) {
  window.history.replaceState({}, '', url);
}

const location = () => window.location.pathname + window.location.search;

beforeEach(() => {
  clearCache.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  visit('/');
  vi.restoreAllMocks();
});

const h1 = (name: string | RegExp) => screen.findByRole('heading', { level: 1, name });

describe('Pages and navigation (WEB-002)', () => {
  const watchedId = TEST_WATCHING.user.id;
  const PAGES: [string, string][] = [
    ['/', 'Devices'],
    ['/partners', 'Partners'],
    ['/passwords', 'Passwords'],
    ['/report', 'My report'],
    [`/report/${watchedId}`, "Bob's report"],
    ['/logs', 'My logs'],
    ['/logs/list', 'My logs'],
    [`/logs/${watchedId}`, "Bob's logs"],
    [`/logs/${watchedId}/list`, "Bob's logs"],
    ['/settings', 'Settings'],
    ['/admin', 'Admin'],
  ];

  it.each(PAGES)('a signed-in user can open %s, headed "%s" (WEB-002)', async (path, heading) => {
    saveSession();
    visit(path);
    render(<App />);
    expect(await h1(heading)).toBeInTheDocument();
    expect(location()).toBe(path);
  });

  it.each(['/logs/gallery', `/logs/${TEST_WATCHING.user.id}/gallery`])(
    'the old gallery link %s still opens the logs (WEB-002)',
    async (path) => {
      saveSession();
      visit(path);
      render(<App />);
      expect(await h1(/logs$/)).toBeInTheDocument();
      // The gallery is the view the alias selects.
      expect(screen.getByRole('link', { name: 'Gallery' })).toHaveClass('is-active');
    },
  );

  it('opens the invite acceptance page from the emailed link (WEB-002)', async () => {
    saveSession();
    visit('/invite-accept?partner_token=abc');
    render(<App />);
    expect(await screen.findByRole('heading', { name: 'Invite accepted' })).toBeInTheDocument();
  });

  it('opens the email verification page from the emailed link (WEB-002)', async () => {
    saveSession();
    server.use(
      http.post(`${API}/email-verification/validate`, () => HttpResponse.json({ ok: true })),
    );
    visit('/verify-email?token=abc');
    render(<App />);
    expect(await screen.findByRole('heading', { name: 'Email updated' })).toBeInTheDocument();
  });

  it('shows the sidebar on every signed-in page (WEB-002, WEB-003)', async () => {
    saveSession();
    visit('/settings');
    render(<App />);
    await h1('Settings');
    expect(screen.getByRole('navigation', { name: 'Primary' })).toBeInTheDocument();
  });

  it('names the page with a heading for an invalid email link (WEB-002, WEB-033)', async () => {
    server.use(
      http.post(`${API}/email-verification/validate`, () =>
        HttpResponse.json({ error: 'Invalid token' }, { status: 400 }),
      ),
    );
    visit('/verify-email?token=bad');
    render(<App />);
    await screen.findByText('This verification link is invalid or has expired.');
    expect(screen.getByRole('heading', { name: 'Verification failed' })).toBeInTheDocument();
  });
});

describe('Page not found (WEB-070, WEB-002)', () => {
  it.each(['/nope', '/partners/extra', '/logs/a/b/c', '/settings/profile', '/devices'])(
    'a signed-in user at %s sees "Page not found" (WEB-070, WEB-002)',
    async (path) => {
      saveSession();
      visit(path);
      render(<App />);
      expect(await h1('Page not found')).toBeInTheDocument();
      expect(screen.getByText(/couldn't find the page you were looking for/i)).toBeInTheDocument();
    },
  );

  it('offers buttons to the dashboard and to Settings (WEB-070)', async () => {
    saveSession();
    visit('/nope');
    render(<App />);
    await h1('Page not found');
    const main = screen.getByRole('main');
    expect(within(main).getByRole('link', { name: 'Go to dashboard' })).toHaveAttribute(
      'href',
      '/',
    );
    expect(within(main).getByRole('link', { name: 'Settings' })).toHaveAttribute(
      'href',
      '/settings',
    );
  });

  it('keeps the sidebar next to the 404 page (WEB-070, WEB-003)', async () => {
    saveSession();
    visit('/nope');
    render(<App />);
    await h1('Page not found');
    expect(screen.getByRole('navigation', { name: 'Primary' })).toBeInTheDocument();
  });
});

describe('Session at startup (WEB-004)', () => {
  it('shows only "Loading…" while it checks for a saved session (WEB-004)', async () => {
    vi.spyOn(Session, 'restore').mockReturnValue(new Promise(() => {}));
    visit('/partners');
    const { container } = render(<App />);
    expect(container).toHaveTextContent(/^Loading…$/);
    // Nothing is redirected while it waits.
    expect(location()).toBe('/partners');
  });

  it('stays signed in after a reload when a key was saved (WEB-004)', async () => {
    saveSession();
    visit('/partners');
    const first = render(<App />);
    await h1('Partners');
    first.unmount();

    render(<App />);
    expect(await h1('Partners')).toBeInTheDocument();
  });

  it('shows the login page when nothing was saved (WEB-004)', async () => {
    visit('/');
    render(<App />);
    expect(await screen.findByRole('heading', { name: 'The Virtue Initiative' })).toBeVisible();
    expect(screen.getByPlaceholderText('Enter your password')).toBeInTheDocument();
  });
});

describe('Signed-out access (WEB-005)', () => {
  it.each(['/login', '/signup', '/forgot-password', '/verify-email?token=abc'])(
    'a signed-out visitor can open %s (WEB-005)',
    async (path) => {
      server.use(
        http.post(`${API}/email-verification/validate`, () => HttpResponse.json({ ok: true })),
      );
      visit(path);
      render(<App />);
      await waitFor(() => {
        expect(
          screen.queryByRole('heading', { name: 'The Virtue Initiative' }) ??
            screen.queryByText(/Verifying…|Email updated/),
        ).toBeInTheDocument();
      });
      expect(location()).toBe(path);
    },
  );

  it.each([
    ['/', '/'],
    ['/partners', '/partners'],
    ['/logs/list?range=week&type=Alert', '/logs/list?range=week&type=Alert'],
    ['/report/user-9?period=weekly', '/report/user-9?period=weekly'],
    ['/invite-accept?partner_token=abc', '/invite-accept?partner_token=abc'],
    ['/nope', '/nope'],
  ])(
    'sends a signed-out visitor from %s to the login page, remembering it (WEB-005)',
    async (path, to) => {
      visit(path);
      render(<App />);
      await screen.findByRole('heading', { name: 'The Virtue Initiative' });
      expect(window.location.pathname).toBe('/login');
      expect(new URLSearchParams(window.location.search).get('to')).toBe(to);
    },
  );

  it('shows the login page, not the page asked for, to a signed-out visitor (WEB-005)', async () => {
    visit('/settings');
    render(<App />);
    await screen.findByRole('heading', { name: 'The Virtue Initiative' });
    expect(screen.queryByRole('heading', { name: 'Settings' })).not.toBeInTheDocument();
  });

  it.each(['/login', '/signup', '/forgot-password'])(
    'redirects a signed-in user who opens %s to the dashboard (WEB-005)',
    async (path) => {
      saveSession();
      visit(path);
      render(<App />);
      expect(await h1('Devices')).toBeInTheDocument();
      expect(location()).toBe('/');
    },
  );
});

describe('Logging in and returning (WEB-005)', () => {
  async function logIn() {
    const user = userEvent.setup();
    await user.type(screen.getByPlaceholderText('you@example.com'), 'test@example.com');
    await user.type(screen.getByPlaceholderText('Enter your password'), 'a long enough password');
    const submit = screen
      .getAllByRole('button', { name: /^log in$/i })
      .find((b) => b.getAttribute('type') === 'submit')!;
    await user.click(submit);
  }

  it('goes to the "to" page after logging in (WEB-005)', async () => {
    vi.spyOn(Session, 'fromLogin').mockResolvedValue(makeFakeSession({ userId: TEST_USER.id }));
    visit('/login?to=%2Fpartners');
    render(<App />);
    await screen.findByRole('heading', { name: 'The Virtue Initiative' });
    await logIn();
    expect(await h1('Partners')).toBeInTheDocument();
    expect(location()).toBe('/partners');
  });

  it('returns to the page, query included, a signed-out visitor first asked for (WEB-005)', async () => {
    vi.spyOn(Session, 'fromLogin').mockResolvedValue(makeFakeSession({ userId: TEST_USER.id }));
    visit(`/logs/${TEST_WATCHING.user.id}?range=week`);
    render(<App />);
    await screen.findByRole('heading', { name: 'The Virtue Initiative' });
    await logIn();
    expect(await h1("Bob's logs")).toBeInTheDocument();
    expect(location()).toBe(`/logs/${TEST_WATCHING.user.id}?range=week`);
  });

  it('goes to the dashboard when there is no "to" (WEB-005)', async () => {
    vi.spyOn(Session, 'fromLogin').mockResolvedValue(makeFakeSession({ userId: TEST_USER.id }));
    visit('/login');
    render(<App />);
    await screen.findByRole('heading', { name: 'The Virtue Initiative' });
    await logIn();
    expect(await h1('Devices')).toBeInTheDocument();
    expect(location()).toBe('/');
  });

  it('keeps "to" when switching between Log in, Sign up and Forgot your password (WEB-005)', async () => {
    const user = userEvent.setup();
    visit('/login?to=%2Fpartners');
    render(<App />);
    await screen.findByRole('heading', { name: 'The Virtue Initiative' });
    const to = () => new URLSearchParams(window.location.search).get('to');

    await user.click(screen.getByRole('button', { name: 'Sign up' }));
    await waitFor(() => expect(window.location.pathname).toBe('/signup'));
    expect(to()).toBe('/partners');

    await user.click(screen.getAllByRole('button', { name: 'Log in' })[0]);
    await waitFor(() => expect(window.location.pathname).toBe('/login'));
    expect(to()).toBe('/partners');

    await user.click(screen.getByRole('link', { name: /forgot your password/i }));
    await waitFor(() => expect(window.location.pathname).toBe('/forgot-password'));
    expect(to()).toBe('/partners');
  });

  it('passes "to" to the server in the sign-up request, so the emailed link returns there (WEB-005)', async () => {
    let body: Record<string, unknown> | undefined;
    server.use(
      http.post(`${API}/signup-request`, async ({ request }) => {
        body = (await request.json()) as Record<string, unknown>;
        return new HttpResponse(null, { status: 204 });
      }),
    );
    const user = userEvent.setup();
    visit('/signup?to=%2Fpartners');
    render(<App />);
    await screen.findByRole('heading', { name: 'The Virtue Initiative' });
    await user.type(screen.getByPlaceholderText('you@example.com'), 'new@example.com');
    await user.click(screen.getByRole('button', { name: /send verification email/i }));
    await waitFor(() => expect(body).toBeDefined());
    expect(body).toMatchObject({ email: 'new@example.com', to: '/partners' });
  });
});

describe('Logging out (WEB-007)', () => {
  async function openedApp() {
    saveSession();
    visit('/partners');
    render(<App />);
    await h1('Partners');
    const user = userEvent.setup();
    return user;
  }

  async function clickLogOut(user: ReturnType<typeof userEvent.setup>) {
    await user.click(screen.getByRole('button', { name: new RegExp(TEST_USER.name!) }));
    await user.click(screen.getByRole('button', { name: 'Log out' }));
  }

  it('calls the logout endpoint, clears the log cache and the saved key, and shows the login page (WEB-007)', async () => {
    const logoutCalls = vi.fn();
    server.use(
      http.post(`${API}/logout`, () => {
        logoutCalls();
        return new HttpResponse(null, { status: 204 });
      }),
    );
    const user = await openedApp();
    expect(localStorage.getItem(WRAPPING_KEY_STORAGE)).not.toBeNull();

    await clickLogOut(user);

    expect(await screen.findByRole('heading', { name: 'The Virtue Initiative' })).toBeVisible();
    expect(logoutCalls).toHaveBeenCalledTimes(1);
    expect(clearCache).toHaveBeenCalled();
    expect(localStorage.getItem(WRAPPING_KEY_STORAGE)).toBeNull();
    expect(window.location.pathname).toBe('/login');
    expect(screen.queryByRole('navigation', { name: 'Primary' })).not.toBeInTheDocument();
  });

  it.each([
    ['an error response', () => HttpResponse.json({ error: 'boom' }, { status: 500 })],
    ['a network failure', () => HttpResponse.error()],
  ])(
    'still logs out locally when the logout request fails with %s (WEB-007)',
    async (_name, respond) => {
      server.use(http.post(`${API}/logout`, respond));
      const user = await openedApp();

      await clickLogOut(user);

      expect(await screen.findByRole('heading', { name: 'The Virtue Initiative' })).toBeVisible();
      expect(clearCache).toHaveBeenCalled();
      expect(localStorage.getItem(WRAPPING_KEY_STORAGE)).toBeNull();
    },
  );

  it('stays logged out after a reload (WEB-007)', async () => {
    const user = await openedApp();
    await clickLogOut(user);
    await screen.findByRole('heading', { name: 'The Virtue Initiative' });
    document.body.innerHTML = '';

    visit('/partners');
    render(<App />);
    expect(await screen.findByRole('heading', { name: 'The Virtue Initiative' })).toBeVisible();
  });
});

describe('Messages across a reload (WEB-008)', () => {
  function leaveMessage(message: string, isError = false) {
    sessionStorage.setItem(MESSAGE_KEY, JSON.stringify({ message, isError }));
  }

  it.each(['Password reset successfully.', 'Your account has been deleted.'])(
    'shows "%s" as a toast on the next page, and only once (WEB-008)',
    async (message) => {
      leaveMessage(message);
      visit('/login');
      const first = render(<App />);
      expect(await screen.findByText(message)).toBeInTheDocument();
      expect(screen.getAllByText(message)).toHaveLength(1);
      expect(sessionStorage.getItem(MESSAGE_KEY)).toBeNull();

      // Moving on to another page does not show it again.
      window.history.pushState({}, '', '/signup');
      window.dispatchEvent(new PopStateEvent('popstate'));
      await screen.findByRole('button', { name: /send verification email/i });
      expect(screen.getAllByText(message)).toHaveLength(1);

      // Nor does a reload.
      first.unmount();
      render(<App />);
      await screen.findByRole('heading', { name: 'The Virtue Initiative' });
      expect(screen.queryByText(message)).not.toBeInTheDocument();
    },
  );

  it('shows the message on a signed-in page too (WEB-008)', async () => {
    saveSession();
    leaveMessage('Password reset successfully.');
    visit('/');
    render(<App />);
    await h1('Devices');
    expect(await screen.findByText('Password reset successfully.')).toBeInTheDocument();
  });

  it('shows "Your account has been deleted." on the login page after deleting the account (WEB-008, WEB-063)', async () => {
    const deleteCalls = vi.fn();
    server.use(
      http.delete(`${API}/user`, ({ request }) => {
        deleteCalls(new URL(request.url).searchParams.get('confirm_email'));
        return new HttpResponse(null, { status: 204 });
      }),
    );
    saveSession();
    visit('/settings');
    render(<App />);
    const user = userEvent.setup();
    await h1('Settings');

    await user.click(await screen.findByRole('button', { name: /^delete account$/i }));
    await user.type(await screen.findByPlaceholderText(TEST_USER.email), TEST_USER.email);
    const confirmButtons = screen.getAllByRole('button', { name: /^delete account$/i });
    await user.click(confirmButtons[confirmButtons.length - 1]);

    expect(await screen.findByText('Your account has been deleted.')).toBeInTheDocument();
    expect(deleteCalls).toHaveBeenCalledWith(TEST_USER.email);
    expect(await screen.findByRole('heading', { name: 'The Virtue Initiative' })).toBeVisible();
  });

  it('ignores a stored message that is not valid (WEB-008)', async () => {
    sessionStorage.setItem(MESSAGE_KEY, '{not json');
    visit('/login');
    render(<App />);
    expect(await screen.findByRole('heading', { name: 'The Virtue Initiative' })).toBeVisible();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });
});

describe('API version (WEB-025)', () => {
  it('calls every API endpoint under the current version prefix (WEB-025)', async () => {
    const paths: string[] = [];
    const listener = ({ request }: { request: Request }) => {
      const url = new URL(request.url);
      if (url.origin === 'http://localhost:8787') paths.push(url.pathname);
    };
    server.events.on('request:start', listener);
    try {
      saveSession();
      const user = userEvent.setup();
      visit('/');
      render(<App />);
      await h1('Devices');
      for (const heading of ['Partners', 'Passwords']) {
        await user.click(
          within(screen.getByRole('navigation', { name: 'Primary' })).getByRole('link', {
            name: new RegExp(`^${heading}`),
          }),
        );
        await h1(heading);
      }
      await user.click(screen.getByRole('button', { name: new RegExp(TEST_USER.name!) }));
      await user.click(screen.getByRole('button', { name: 'Log out' }));
      await screen.findByRole('heading', { name: 'The Virtue Initiative' });
    } finally {
      server.events.removeListener('request:start', listener);
    }

    expect(paths.length).toBeGreaterThan(4);
    expect(paths).toContain(`/${CURRENT_API_VERSION}/user`);
    expect(paths).toContain(`/${CURRENT_API_VERSION}/device`);
    expect(paths).toContain(`/${CURRENT_API_VERSION}/logout`);
    expect(paths.filter((p) => !p.startsWith(`/${CURRENT_API_VERSION}/`))).toEqual([]);
  });

  it('the current version is a /vX.Y prefix (WEB-025)', () => {
    expect(CURRENT_API_VERSION).toMatch(/^v\d+(\.\d+)?$/);
  });
});
