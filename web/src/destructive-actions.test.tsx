// WEB-020: an action that deletes data or ends a relationship asks first, in a dialog that
// names what is affected, and leaves everything as it was if it fails. Every such action
// in the app is a row of the table below, and the same rules are checked against each.
import { describe, expect, it } from 'vitest';
import { screen, waitFor, within } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { server } from './mocks/server';
import {
  TEST_DEVICES,
  TEST_FEATURE_FLAG,
  TEST_LOCKED_PASSWORD,
  TEST_USER,
  TEST_WATCHER,
  TEST_WATCHING,
} from './mocks/fixtures';
import { makeFakeSession, renderWithClient } from './test-utils';
import { encryptForPublicKey, generateUserKeyPair } from './utils/api/crypto';
import type { Session } from './utils/api/session';
import { Admin } from './pages/Admin/index';
import { Devices } from './pages/Devices/index';
import { Partners } from './pages/Partners/index';
import { Passwords } from './pages/Passwords/index';
import { Settings } from './pages/Settings/index';

const API = `http://localhost:8787/${CURRENT_API_VERSION}`;
type User = ReturnType<typeof userEvent.setup>;
type Method = 'get' | 'post' | 'patch' | 'put' | 'delete';

const dialogWith = (text: string | RegExp) => screen.getByText(text).closest('dialog')!;
const button = (within_: HTMLElement | null, name: string | RegExp) =>
  within(within_ ?? document.body).getByRole('button', { name });
const cardOf = (text: string) => screen.getByText(text).closest('.vi-card') as HTMLElement;

type Action = {
  name: string;
  method: Method;
  route: string;
  /** Mocks and session the page needs. */
  prepare?: () => Promise<Session | undefined> | Session | undefined | void;
  render: (session?: Session) => void;
  /** Clicks through to the confirmation and returns its dialog. */
  open: (user: User) => Promise<HTMLElement>;
  /** What the dialog must say is affected. */
  affects: (string | RegExp)[];
  confirm: string;
  /** While the request runs: the element that says what is happening, and the controls that must be off. */
  pending: () => { label: HTMLElement; disabled: HTMLElement[] };
  /** The thing that must still be on the page after a failure. */
  remains: () => HTMLElement | null;
  /** Whether the confirmation is still open while the request runs. */
  dialogStaysOpen: boolean;
  /** For a confirmation that closes: the control that starts the action again. */
  retry?: () => HTMLElement;
  /** Extra steps before the confirm button is enabled. */
  beforeConfirm?: (user: User, dialog: HTMLElement) => Promise<void>;
  /** Where the failure's error text shows. */
  failure: 'toast' | 'dialog';
};

const watcherSessionKeys = async () => {
  const { publicKey, privateKeyHandle } = await generateUserKeyPair();
  const wrapped = await encryptForPublicKey(publicKey, new TextEncoder().encode('hunter2'));
  server.use(
    http.get(`${API}/user`, () =>
      HttpResponse.json({ ...TEST_USER, pub_key: publicKey.toBase64() }),
    ),
    http.post(`${API}/locked-password/:id/reveal`, () =>
      HttpResponse.json({ wrapped_value: wrapped.toBase64(), accessed_at: Date.now() }),
    ),
  );
  return makeFakeSession({ privateKey: privateKeyHandle });
};

const deletedPassword = () =>
  server.use(
    http.get(`${API}/locked-password`, () =>
      HttpResponse.json([{ ...TEST_LOCKED_PASSWORD, deleted_at: Date.now() }]),
    ),
  );

const ACTIONS: Action[] = [
  {
    name: 'deleting a device',
    method: 'delete',
    route: '/device/:id',
    render: () => renderWithClient(<Devices />),
    open: async (user) => {
      await user.click((await screen.findAllByRole('button', { name: 'Edit' }))[0]);
      await user.click(screen.getByRole('button', { name: 'Delete device permanently' }));
      return dialogWith(/Delete "My Laptop"\?/);
    },
    affects: ['My Laptop', /permanently removes its logs and uploads/, /partners will be notified/],
    confirm: 'Delete device',
    pending: () => {
      const dialog = dialogWith(/Delete "My Laptop"\?/);
      return { label: button(dialog, 'Deleting…'), disabled: [button(dialog, 'Cancel')] };
    },
    remains: () => screen.queryByText(TEST_DEVICES[0].name),
    dialogStaysOpen: true,
    failure: 'toast',
  },
  {
    name: 'removing a person you monitor',
    method: 'delete',
    route: '/partner/:id',
    render: () => renderWithClient(<Partners />),
    open: async (user) => {
      await screen.findByText(TEST_WATCHING.user.name!);
      await user.click(button(cardOf(TEST_WATCHING.user.name!), 'Remove'));
      return dialogWith(`Remove ${TEST_WATCHING.user.name}?`);
    },
    affects: [TEST_WATCHING.user.name!, /partner will be notified/],
    confirm: 'Remove partner',
    pending: () => {
      const remove = button(cardOf(TEST_WATCHING.user.name!), 'Removing…');
      return { label: remove, disabled: [remove] };
    },
    remains: () => screen.queryByText(TEST_WATCHING.user.name!),
    dialogStaysOpen: false,
    retry: () => button(cardOf(TEST_WATCHING.user.name!), 'Remove'),
    failure: 'toast',
  },
  {
    name: 'removing a person who monitors you',
    method: 'delete',
    route: '/partner/:id',
    render: () => renderWithClient(<Partners />),
    open: async (user) => {
      await screen.findByText(TEST_WATCHER.user.name!);
      await user.click(button(cardOf(TEST_WATCHER.user.name!), 'Remove'));
      return dialogWith(`Remove ${TEST_WATCHER.user.name}?`);
    },
    affects: [TEST_WATCHER.user.name!, /partner will be notified/],
    confirm: 'Remove partner',
    pending: () => {
      const remove = button(cardOf(TEST_WATCHER.user.name!), 'Removing…');
      return { label: remove, disabled: [remove] };
    },
    remains: () => screen.queryByText(TEST_WATCHER.user.name!),
    dialogStaysOpen: false,
    retry: () => button(cardOf(TEST_WATCHER.user.name!), 'Remove'),
    failure: 'toast',
  },
  {
    name: 'removing a pending invite',
    method: 'delete',
    route: '/partner/:id',
    prepare: () => {
      server.use(
        http.get(`${API}/partner`, () =>
          HttpResponse.json({
            watchers: [{ ...TEST_WATCHER, status: 'pending' }],
            watching: [],
          }),
        ),
      );
    },
    render: () => renderWithClient(<Partners />),
    open: async (user) => {
      const card = await waitFor(() => cardOf(TEST_WATCHER.user.name!));
      expect(within(card).getByText('Pending')).toBeInTheDocument();
      await user.click(button(card, 'Remove'));
      return dialogWith(`Remove ${TEST_WATCHER.user.name}?`);
    },
    affects: [TEST_WATCHER.user.name!, 'This will cancel the pending partner relationship.'],
    confirm: 'Remove partner',
    pending: () => {
      const remove = button(cardOf(TEST_WATCHER.user.name!), 'Removing…');
      return { label: remove, disabled: [remove] };
    },
    remains: () => screen.queryByText(TEST_WATCHER.user.name!),
    dialogStaysOpen: false,
    retry: () => button(cardOf(TEST_WATCHER.user.name!), 'Remove'),
    failure: 'toast',
  },
  {
    name: 'revealing a locked password for the first time',
    method: 'post',
    route: '/locked-password/:id/reveal',
    prepare: watcherSessionKeys,
    render: (session) => renderWithClient(<Passwords />, undefined, session),
    open: async (user) => {
      await user.click(await screen.findByRole('button', { name: 'Reveal' }));
      return dialogWith(`Reveal "${TEST_LOCKED_PASSWORD.label}"?`);
    },
    affects: [
      TEST_LOCKED_PASSWORD.label,
      /permanently flags this password/,
      /emails every partner/,
      /can't be undone/,
    ],
    confirm: 'Reveal anyway',
    pending: () => {
      const reveal = button(cardOf(TEST_LOCKED_PASSWORD.label), 'Revealing…');
      return { label: reveal, disabled: [reveal] };
    },
    remains: () => screen.queryByText(TEST_LOCKED_PASSWORD.label),
    dialogStaysOpen: false,
    retry: () => button(cardOf(TEST_LOCKED_PASSWORD.label), 'Reveal'),
    failure: 'toast',
  },
  {
    name: 'deleting a locked password',
    method: 'delete',
    route: '/locked-password/:id',
    render: () => renderWithClient(<Passwords />),
    open: async (user) => {
      await user.click(await screen.findByRole('button', { name: 'Delete' }));
      return dialogWith(`Delete "${TEST_LOCKED_PASSWORD.label}"?`);
    },
    affects: [TEST_LOCKED_PASSWORD.label, /Recently deleted for 7 days/],
    confirm: 'Delete',
    pending: () => {
      const dialog = dialogWith(`Delete "${TEST_LOCKED_PASSWORD.label}"?`);
      return { label: button(dialog, 'Deleting…'), disabled: [button(dialog, 'Cancel')] };
    },
    remains: () => screen.queryByText(TEST_LOCKED_PASSWORD.label),
    dialogStaysOpen: true,
    failure: 'toast',
  },
  {
    name: 'permanently deleting a locked password',
    method: 'delete',
    route: '/locked-password/:id/permanent',
    prepare: deletedPassword,
    render: () => renderWithClient(<Passwords />),
    open: async (user) => {
      await user.click(await screen.findByRole('button', { name: 'Delete permanently' }));
      return dialogWith(`Delete "${TEST_LOCKED_PASSWORD.label}" permanently?`);
    },
    affects: [TEST_LOCKED_PASSWORD.label, "This can't be undone."],
    confirm: 'Delete permanently',
    pending: () => {
      const dialog = dialogWith(`Delete "${TEST_LOCKED_PASSWORD.label}" permanently?`);
      return { label: button(dialog, 'Deleting…'), disabled: [button(dialog, 'Cancel')] };
    },
    remains: () => screen.queryByText(TEST_LOCKED_PASSWORD.label),
    dialogStaysOpen: true,
    failure: 'toast',
  },
  {
    name: 'deleting the account',
    method: 'delete',
    route: '/user',
    render: () => renderWithClient(<Settings />),
    open: async (user) => {
      await user.click(await screen.findByRole('button', { name: 'Delete account' }));
      return dialogWith(/permanently removes your account and all associated data/);
    },
    affects: [TEST_USER.email, /permanently removes your account and all associated data/],
    confirm: 'Delete account',
    beforeConfirm: async (user, dialog) => {
      expect(button(dialog, 'Delete account')).toBeDisabled();
      await user.type(within(dialog).getByPlaceholderText(TEST_USER.email), TEST_USER.email);
    },
    pending: () => {
      const dialog = dialogWith(/permanently removes your account and all associated data/);
      return { label: button(dialog, 'Deleting…'), disabled: [button(dialog, 'Cancel')] };
    },
    remains: () => screen.queryByRole('heading', { name: 'Settings' }),
    dialogStaysOpen: true,
    failure: 'dialog',
  },
  {
    name: 'deleting a feature flag',
    method: 'delete',
    route: '/admin/flags/:key',
    render: () => renderWithClient(<Admin />),
    open: async (user) => {
      await screen.findByRole('heading', { name: TEST_FEATURE_FLAG.key });
      await user.click(screen.getByRole('button', { name: 'Delete flag' }));
      return dialogWith(/turns off for everyone/);
    },
    affects: [TEST_FEATURE_FLAG.key, /turns off for everyone/, /overrides are removed/],
    confirm: 'Delete flag',
    pending: () => {
      const dialog = dialogWith(/turns off for everyone/);
      return { label: button(dialog, 'Deleting…'), disabled: [button(dialog, 'Cancel')] };
    },
    remains: () => screen.queryByRole('heading', { name: TEST_FEATURE_FLAG.key }),
    dialogStaysOpen: true,
    failure: 'toast',
  },
];

/** Answers the action's request once `release` is called, and counts the requests. */
function interceptRequest(action: Action, respond: () => Response) {
  let release: () => void = () => {};
  const gate = new Promise<void>((resolve) => (release = resolve));
  const requests: string[] = [];
  server.use(
    http[action.method](`${API}${action.route}`, async ({ request }) => {
      requests.push(request.url);
      await gate;
      return respond();
    }),
  );
  return { requests, release };
}

const ok = () => new HttpResponse(null, { status: 204 });
const refused = () => HttpResponse.json({ error: 'The server said no' }, { status: 500 });

describe.each(ACTIONS)('Destructive action: $name (WEB-020)', (action) => {
  /** Renders the page, with the action's request held until `release`. */
  async function start(respond: () => Response) {
    const session = await action.prepare?.();
    const held = interceptRequest(action, respond);
    action.render(session instanceof Object ? session : undefined);
    return { user: userEvent.setup(), ...held };
  }

  it('asks for confirmation in a dialog that names what is affected, with Cancel and a danger button, before sending anything (WEB-020)', async () => {
    const { user, requests, release } = await start(ok);
    const dialog = await action.open(user);

    expect(dialog).toHaveAttribute('open');
    for (const text of action.affects) {
      expect(within(dialog).getByText(text, { exact: false })).toBeInTheDocument();
    }
    expect(within(dialog).getByRole('button', { name: 'Cancel' })).toBeEnabled();
    expect(within(dialog).getByRole('button', { name: action.confirm })).toHaveClass(
      'vi-btn--danger',
    );
    expect(requests).toEqual([]);
    release();
  });

  it('does nothing when the user cancels (WEB-020)', async () => {
    const { user, requests, release } = await start(ok);
    const dialog = await action.open(user);
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));

    expect(dialog).not.toHaveAttribute('open');
    expect(requests).toEqual([]);
    expect(action.remains()).toBeInTheDocument();
    release();
  });

  it('sends the request only after confirming, and shows what is happening while it runs (WEB-020)', async () => {
    const { user, requests, release } = await start(ok);
    const dialog = await action.open(user);
    await action.beforeConfirm?.(user, dialog);
    await user.click(within(dialog).getByRole('button', { name: action.confirm }));

    await waitFor(() => expect(requests).toHaveLength(1));
    const { label, disabled } = action.pending();
    expect(label).toBeDisabled();
    for (const control of disabled) expect(control).toBeDisabled();
    if (action.dialogStaysOpen) expect(dialog).toHaveAttribute('open');
    release();
  });

  it('shows the error and leaves everything as it was when the request fails (WEB-020)', async () => {
    const { user, requests, release } = await start(refused);
    const dialog = await action.open(user);
    await action.beforeConfirm?.(user, dialog);
    await user.click(within(dialog).getByRole('button', { name: action.confirm }));
    await waitFor(() => expect(requests).toHaveLength(1));
    // The item is still there while the request is in flight, not removed ahead of the server.
    expect(action.remains()).toBeInTheDocument();
    release();

    expect(
      await screen.findByText(
        'Something went wrong, please try again. If this continues, contact help@virtueinitiative.org.',
      ),
    ).toBeInTheDocument();
    expect(action.remains()).toBeInTheDocument();
    if (action.failure === 'dialog') expect(dialog).toHaveAttribute('open');
    // The controls come back, so the user can try again or cancel.
    await waitFor(() => {
      if (action.dialogStaysOpen) {
        expect(within(dialog).getByRole('button', { name: 'Cancel' })).toBeEnabled();
        expect(within(dialog).getByRole('button', { name: action.confirm })).toBeEnabled();
      } else {
        expect(action.retry).toBeDefined();
        expect(action.retry!()).toBeEnabled();
      }
    });
  });
});

describe('Actions that do not ask for confirmation (WEB-020)', () => {
  it('restoring a locked password acts at once (WEB-020)', async () => {
    deletedPassword();
    const restored = new Promise<string>((resolve) =>
      server.use(
        http.post(`${API}/locked-password/:id/restore`, ({ params }) => {
          resolve(params.id as string);
          return ok();
        }),
      ),
    );
    const user = userEvent.setup();
    renderWithClient(<Passwords />);
    await user.click(await screen.findByRole('button', { name: 'Restore' }));

    expect(await restored).toBe(TEST_LOCKED_PASSWORD.id);
    expect(document.querySelector('dialog[open]')).toBeNull();
  });

  it('removing a feature flag override acts at once (WEB-020)', async () => {
    const bodies: unknown[] = [];
    server.use(
      http.put(`${API}/admin/flags/:key/users`, async ({ request }) => {
        bodies.push(await request.json());
        return HttpResponse.json(TEST_FEATURE_FLAG);
      }),
    );
    const user = userEvent.setup();
    renderWithClient(<Admin />);
    await user.click(
      await screen.findByRole('button', { name: /remove override for tester@example.com/i }),
    );

    await waitFor(() => expect(bodies).toEqual([{ email: 'tester@example.com', enabled: null }]));
    expect(document.querySelector('dialog[open]')).toBeNull();
  });
});
