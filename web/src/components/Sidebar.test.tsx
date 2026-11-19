import { act, screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { server } from '../mocks/server';
import { TEST_USER, TEST_WATCHING } from '../mocks/fixtures';
import { renderWithClient } from '../test-utils';
import { Sidebar } from './Sidebar';

describe('Sidebar (WEB-003)', () => {
  it('renders the primary nav links and user account button', async () => {
    renderWithClient(<Sidebar />);

    await waitFor(() => {
      expect(screen.getByRole('link', { name: /^devices/i })).toBeInTheDocument();
    });
    expect(screen.getByRole('link', { name: /^partners/i })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /my logs/i })).toBeInTheDocument();
    // Settings and Log out are in the user account popup; verify the trigger button exists
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /test user/i })).toBeInTheDocument();
    });
  });

  it('renders a logs sub-item per accepted partner', async () => {
    renderWithClient(<Sidebar />);

    await waitFor(() => {
      expect(
        screen.getByRole('link', { name: new RegExp(`${TEST_WATCHING.user.name}'s logs`, 'i') }),
      ).toBeInTheDocument();
    });
    expect(
      screen.getByRole('link', { name: new RegExp(`${TEST_WATCHING.user.name}'s logs`, 'i') }),
    ).toHaveAttribute('href', `/logs/${TEST_WATCHING.user.id}`);
  });

  it('renders a report sub-item for me and per accepted partner', async () => {
    renderWithClient(<Sidebar />);

    await waitFor(() => {
      expect(screen.getByRole('link', { name: /my report/i })).toHaveAttribute('href', '/report');
    });
    expect(
      await screen.findByRole('link', {
        name: new RegExp(`${TEST_WATCHING.user.name}'s report`, 'i'),
      }),
    ).toHaveAttribute('href', `/report/${TEST_WATCHING.user.id}`);
  });
});

describe('Sidebar drawer and account menu (WEB-003)', () => {
  type Listener = (e: { matches: boolean }) => void;

  /** Stands in for the browser's narrow-screen media query, which jsdom does not implement. */
  function mockNarrowScreen() {
    const listeners = new Set<Listener>();
    const query = {
      matches: true,
      addEventListener: (_: string, l: Listener) => listeners.add(l),
      removeEventListener: (_: string, l: Listener) => listeners.delete(l),
    };
    vi.stubGlobal('matchMedia', () => query);
    return {
      widen() {
        query.matches = false;
        act(() => listeners.forEach((l) => l({ matches: false })));
      },
    };
  }

  afterEach(() => {
    vi.unstubAllGlobals();
    document.body.style.overflow = '';
  });

  function renderWithMain() {
    renderWithClient(
      <>
        <Sidebar />
        <main class="app-main">
          <button type="button">Page content</button>
        </main>
      </>,
    );
  }

  const openMenu = async () => {
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Open navigation menu' }));
    return user;
  };
  const sidebar = () => document.getElementById('app-sidebar')!;
  const main = () => document.querySelector('.app-main')!;

  it('closes the menu when the window widens to the full sidebar (WEB-003)', async () => {
    const screenSize = mockNarrowScreen();
    renderWithMain();
    await openMenu();
    expect(sidebar()).toHaveClass('is-open');

    screenSize.widen();

    await waitFor(() => expect(sidebar()).not.toHaveClass('is-open'));
    expect(document.querySelector('.app-drawer-backdrop')).not.toHaveClass('is-open');
    expect(main()).not.toHaveAttribute('inert');
  });

  it('closes the menu on Escape and returns focus to the menu button (WEB-003)', async () => {
    mockNarrowScreen();
    renderWithMain();
    const user = await openMenu();
    expect(sidebar()).toHaveClass('is-open');

    await user.keyboard('{Escape}');

    expect(sidebar()).not.toHaveClass('is-open');
    expect(screen.getByRole('button', { name: 'Open navigation menu' })).toHaveFocus();
  });

  it('keeps the keyboard and the scroll off the page behind the open menu (WEB-003)', async () => {
    mockNarrowScreen();
    renderWithMain();
    const user = await openMenu();

    expect(main()).toHaveAttribute('inert');
    expect(document.querySelector('.sidebar-mobile-bar')).toHaveAttribute('inert');
    expect(document.body.style.overflow).toBe('hidden');

    await user.keyboard('{Escape}');
    expect(main()).not.toHaveAttribute('inert');
    expect(document.body.style.overflow).toBe('');
  });

  it('lists each brand link and close button once in the tab order of the open menu (WEB-003)', async () => {
    mockNarrowScreen();
    renderWithMain();
    await openMenu();

    const tabbable = [...document.querySelectorAll<HTMLElement>('a, button')].filter(
      (el) => el.tabIndex >= 0 && !el.closest('[inert]'),
    );
    const names = tabbable.map((el) => el.getAttribute('aria-label') ?? el.textContent);
    expect(names.filter((n) => n === 'Close navigation menu')).toHaveLength(1);
    expect(names.filter((n) => n === 'The Virtue Initiative')).toHaveLength(1);
  });

  it('keeps the account menu items out of the keyboard order until it is opened (WEB-003)', async () => {
    renderWithClient(<Sidebar />);
    const user = userEvent.setup();
    const account = await screen.findByRole('button', { name: new RegExp(TEST_USER.name!) });
    const logOut = () => screen.getByText('Log out').closest('button')!;

    expect(logOut().closest('[inert]')).not.toBeNull();
    await user.click(account);
    expect(logOut().closest('[inert]')).toBeNull();
  });

  it('closes the account menu on Escape and returns focus to the account button (WEB-003)', async () => {
    renderWithClient(<Sidebar />);
    const user = userEvent.setup();
    const account = await screen.findByRole('button', { name: new RegExp(TEST_USER.name!) });
    await user.click(account);
    expect(account).toHaveAttribute('aria-expanded', 'true');

    await user.keyboard('{Escape}');

    expect(account).toHaveAttribute('aria-expanded', 'false');
    expect(account).toHaveFocus();
  });

  it('shows whole characters as the avatar initials, even when the name ends in an emoji (WEB-003)', async () => {
    server.use(
      http.get(`http://localhost:8787/${CURRENT_API_VERSION}/user`, () =>
        HttpResponse.json({ ...TEST_USER, name: 'Bob \u{1F600}' }),
      ),
    );
    renderWithClient(<Sidebar />);

    const account = await screen.findByRole('button', { name: /Bob/ });
    expect(account.querySelector('.vi-avatar__initials')).toHaveTextContent('B\u{1F600}');
  });
});
