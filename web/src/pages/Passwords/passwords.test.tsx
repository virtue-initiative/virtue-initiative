import { screen, waitFor } from '@testing-library/preact';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { describe, expect, it } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_LOCKED_PASSWORD, TEST_USER } from '../../mocks/fixtures';
import { renderWithClient, makeFakeSession } from '../../test-utils';
import { encryptForPublicKey, generateUserKeyPair } from '../../utils/api/crypto';
import { Passwords } from './index';

const BASE = `http://localhost:8787/${CURRENT_API_VERSION}`;
const textEncoder = new TextEncoder();

describe('Passwords — list', () => {
  it('shows "Passwords" page heading', async () => {
    renderWithClient(<Passwords />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /^passwords$/i })).toBeInTheDocument();
    });
  });

  it('renders locked password labels once loaded', async () => {
    renderWithClient(<Passwords />);
    await waitFor(() => {
      expect(screen.getByText('Screen Time passcode')).toBeInTheDocument();
    });
  });

  it('shows "No locked passwords" once loaded with an empty list', async () => {
    server.use(http.get(`${BASE}/locked-password`, () => HttpResponse.json([])));

    renderWithClient(<Passwords />);

    await waitFor(() => {
      expect(screen.getByText('No locked passwords')).toBeInTheDocument();
    });
  });

  it('shows a Recently deleted section only when a deleted entry exists', async () => {
    server.use(
      http.get(`${BASE}/locked-password`, () =>
        HttpResponse.json([{ ...TEST_LOCKED_PASSWORD, deleted_at: Date.now() }]),
      ),
    );

    renderWithClient(<Passwords />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /recently deleted/i })).toBeInTheDocument();
    });
  });
});

describe('Passwords — create', () => {
  it('opens the Add password dialog', async () => {
    const user = userEvent.setup();
    renderWithClient(<Passwords />);

    const addBtn = await screen.findByRole('button', { name: /add password/i });
    await user.click(addBtn);

    expect(screen.getByRole('heading', { name: /add a locked password/i })).toBeInTheDocument();
  });

  it('POSTs an encrypted value on submit', async () => {
    const { publicKey } = await generateUserKeyPair();
    server.use(
      http.get(`${BASE}/user`, () =>
        HttpResponse.json({ ...TEST_USER, pub_key: publicKey.toBase64() }),
      ),
    );
    let postBody: unknown;
    server.use(
      http.post(`${BASE}/locked-password`, async ({ request }) => {
        postBody = await request.json();
        return HttpResponse.json({ id: 'new-password-1' });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Passwords />);

    await user.click(await screen.findByRole('button', { name: /add password/i }));
    await user.type(screen.getByLabelText(/^name$/i), 'My secret');
    await user.type(screen.getByLabelText(/secret value/i), 'hunter2');
    const submitBtns = screen.getAllByRole('button', { name: /add password/i });
    await user.click(submitBtns[submitBtns.length - 1]);

    await waitFor(() => {
      expect(postBody).toMatchObject({ label: 'My secret' });
      expect(typeof (postBody as { wrapped_value: string }).wrapped_value).toBe('string');
    });
  });
});

describe('Passwords — reveal', () => {
  it('warns before the first reveal, then decrypts and displays the value', async () => {
    const { publicKey, privateKeyHandle } = await generateUserKeyPair();
    const wrapped = await encryptForPublicKey(publicKey, textEncoder.encode('hunter2'));

    server.use(
      http.get(`${BASE}/user`, () =>
        HttpResponse.json({ ...TEST_USER, pub_key: publicKey.toBase64() }),
      ),
    );
    server.use(
      http.post(`${BASE}/locked-password/:id/reveal`, () =>
        HttpResponse.json({ wrapped_value: wrapped.toBase64(), accessed_at: Date.now() }),
      ),
    );

    const user = userEvent.setup();
    renderWithClient(<Passwords />, undefined, makeFakeSession({ privateKey: privateKeyHandle }));

    await user.click(await screen.findByRole('button', { name: /^reveal$/i }));
    expect(screen.getByRole('button', { name: /reveal anyway/i })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: /reveal anyway/i }));

    await waitFor(() => {
      expect(screen.getByText('hunter2')).toBeInTheDocument();
    });
  });

  it('reveals an already-accessed entry without a warning dialog', async () => {
    const { publicKey, privateKeyHandle } = await generateUserKeyPair();
    const wrapped = await encryptForPublicKey(publicKey, textEncoder.encode('hunter2'));

    server.use(
      http.get(`${BASE}/user`, () =>
        HttpResponse.json({ ...TEST_USER, pub_key: publicKey.toBase64() }),
      ),
    );
    server.use(
      http.get(`${BASE}/locked-password`, () =>
        HttpResponse.json([{ ...TEST_LOCKED_PASSWORD, accessed_at: Date.now() - 1000 }]),
      ),
    );
    server.use(
      http.post(`${BASE}/locked-password/:id/reveal`, () =>
        HttpResponse.json({ wrapped_value: wrapped.toBase64(), accessed_at: Date.now() - 1000 }),
      ),
    );

    const user = userEvent.setup();
    renderWithClient(<Passwords />, undefined, makeFakeSession({ privateKey: privateKeyHandle }));

    await user.click(await screen.findByRole('button', { name: /^reveal$/i }));

    expect(screen.queryByRole('button', { name: /reveal anyway/i })).not.toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText('hunter2')).toBeInTheDocument();
    });
  });
});

describe('Passwords — delete, restore, permanent delete', () => {
  it('soft-deletes after confirmation', async () => {
    let deletedId: string | undefined;
    server.use(
      http.delete(`${BASE}/locked-password/:id`, ({ params }) => {
        deletedId = params.id as string;
        return new HttpResponse(null, { status: 204 });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Passwords />);

    await user.click(await screen.findByRole('button', { name: /^delete$/i }));
    const confirmBtns = screen.getAllByRole('button', { name: /^delete$/i });
    await user.click(confirmBtns[confirmBtns.length - 1]);

    await waitFor(() => {
      expect(deletedId).toBe(TEST_LOCKED_PASSWORD.id);
    });
  });

  it('restores a deleted entry', async () => {
    server.use(
      http.get(`${BASE}/locked-password`, () =>
        HttpResponse.json([{ ...TEST_LOCKED_PASSWORD, deleted_at: Date.now() }]),
      ),
    );
    let restoredId: string | undefined;
    server.use(
      http.post(`${BASE}/locked-password/:id/restore`, ({ params }) => {
        restoredId = params.id as string;
        return new HttpResponse(null, { status: 204 });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Passwords />);

    await user.click(await screen.findByRole('button', { name: /^restore$/i }));

    await waitFor(() => {
      expect(restoredId).toBe(TEST_LOCKED_PASSWORD.id);
    });
  });

  it('permanently deletes after confirmation', async () => {
    server.use(
      http.get(`${BASE}/locked-password`, () =>
        HttpResponse.json([{ ...TEST_LOCKED_PASSWORD, deleted_at: Date.now() }]),
      ),
    );
    let permanentlyDeletedId: string | undefined;
    server.use(
      http.delete(`${BASE}/locked-password/:id/permanent`, ({ params }) => {
        permanentlyDeletedId = params.id as string;
        return new HttpResponse(null, { status: 204 });
      }),
    );

    const user = userEvent.setup();
    renderWithClient(<Passwords />);

    await user.click(await screen.findByRole('button', { name: /delete permanently/i }));
    const confirmBtns = screen.getAllByRole('button', { name: /delete permanently/i });
    await user.click(confirmBtns[confirmBtns.length - 1]);

    await waitFor(() => {
      expect(permanentlyDeletedId).toBe(TEST_LOCKED_PASSWORD.id);
    });
  });
});
