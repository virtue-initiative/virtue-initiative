// WEB-021: a second submit, however quick, does not send the form again, and a dialog does not
// close on Escape while its form is submitting.
import { describe, expect, it, vi } from 'vitest';
import { screen, waitFor, within } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { server } from './mocks/server';
import { TEST_USER } from './mocks/fixtures';
import { makeFakeSession, renderWithClient } from './test-utils';
import { generateUserKeyPair } from './utils/api/crypto';
import { Passwords } from './pages/Passwords/index';
import { Settings } from './pages/Settings/index';
import { Sidebar } from './components/Sidebar';

const API = `http://localhost:8787/${CURRENT_API_VERSION}`;

/** Submits a form twice before Preact can re-render, which act() would flush in between. */
const submitTwice = (form: HTMLFormElement) => {
  form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
};

function holdRequest(method: 'post', path: string, respond: () => Response) {
  let release: () => void = () => {};
  const gate = new Promise<void>((resolve) => (release = resolve));
  const requests: unknown[] = [];
  server.use(
    http[method](`${API}${path}`, async ({ request }) => {
      requests.push(await request.clone().text());
      await gate;
      return respond();
    }),
  );
  return { requests, release };
}

async function openAddPassword() {
  const { publicKey } = await generateUserKeyPair();
  server.use(
    http.get(`${API}/user`, () =>
      HttpResponse.json({ ...TEST_USER, pub_key: publicKey.toBase64() }),
    ),
  );
  const user = userEvent.setup();
  renderWithClient(<Passwords />);
  await user.click(await screen.findByRole('button', { name: /add password/i }));
  await user.type(screen.getByLabelText(/^name$/i), 'My secret');
  await user.type(screen.getByLabelText(/secret value/i), 'hunter2');
  const dialog = screen.getByRole('heading', { name: /add a locked password/i }).closest('dialog')!;
  return { user, dialog, form: dialog.querySelector('form')! };
}

describe('Double submit and Escape while saving (WEB-021)', () => {
  it('adds a password once when the form is submitted twice in the same tick (WEB-021)', async () => {
    const { requests, release } = holdRequest('post', '/locked-password', () =>
      HttpResponse.json({ id: 'new-password-1' }),
    );
    const { form } = await openAddPassword();
    submitTwice(form);

    await waitFor(() => expect(requests).toHaveLength(1));
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(requests).toHaveLength(1);
    release();
  });

  it('keeps the Add password dialog open on Escape while it is saving (WEB-021)', async () => {
    const { requests, release } = holdRequest('post', '/locked-password', () =>
      HttpResponse.json({ id: 'new-password-1' }),
    );
    const { user, dialog } = await openAddPassword();
    await user.click(within(dialog).getByRole('button', { name: 'Add password' }));
    await waitFor(() => expect(requests).toHaveLength(1));
    await screen.findByRole('button', { name: 'Saving…' });

    // The browser fires a cancelable `cancel` event on Escape and closes the dialog unless it is prevented.
    const cancel = new Event('cancel', { cancelable: true });
    dialog.dispatchEvent(cancel);
    expect(cancel.defaultPrevented).toBe(true);
    release();
  });

  it('lets Escape close the Add password dialog when it is not saving (WEB-021)', async () => {
    const { dialog } = await openAddPassword();
    const cancel = new Event('cancel', { cancelable: true });
    dialog.dispatchEvent(cancel);
    expect(cancel.defaultPrevented).toBe(false);
  });

  it('changes the password once when the form is submitted twice in the same tick (WEB-021)', async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    const session = makeFakeSession({
      changePassword: vi.fn().mockImplementation(() => gate),
    });
    renderWithClient(<Settings />, undefined, session);
    const user = userEvent.setup();
    await user.type(await screen.findByLabelText('Current password'), 'current-pass-1');
    await user.type(screen.getByLabelText('New password'), 'brand-new-pass-1');
    await user.type(screen.getByLabelText('Confirm new password'), 'brand-new-pass-1');
    submitTwice(screen.getByLabelText('Current password').closest('form')!);

    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(session.changePassword).toHaveBeenCalledTimes(1);
    release();
  });

  it('sends one bug report when the form is submitted twice in the same tick (WEB-021)', async () => {
    const { requests, release } = holdRequest(
      'post',
      '/bug-report',
      () => new HttpResponse(null, { status: 204 }),
    );
    const user = userEvent.setup();
    renderWithClient(<Sidebar />);
    await user.click(await screen.findByRole('button', { name: new RegExp(TEST_USER.name!) }));
    await user.click(screen.getByRole('button', { name: 'Report an issue' }));
    await user.type(screen.getByPlaceholderText(/describe what happened/i), 'Broken');
    submitTwice(screen.getByPlaceholderText(/describe what happened/i).closest('form')!);

    await waitFor(() => expect(requests).toHaveLength(1));
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(requests).toHaveLength(1);
    release();
  });
});
