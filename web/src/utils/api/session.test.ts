import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { server } from '../../mocks/server';
import { TEST_USER } from '../../mocks/fixtures';
import {
  derivePasswordMaterial,
  encryptData,
  encryptForPublicKey,
  generateRandomKeyBytes,
  generateUserKeyPair,
  unwrapBatchKey,
} from './crypto';
import { Session } from './session';

vi.mock('../cache/client', () => ({ cacheClient: null }));

const BASE = `http://localhost:8787/${CURRENT_API_VERSION}`;
// Real argon2id, but cheap enough to run several derivations per test.
const PARAMS = {
  version: 'argon2id-v1',
  algorithm: 'argon2id',
  salt_length: 16,
  memory_cost_kib: 1024,
  time_cost: 1,
  parallelism: 1,
  hkdf_hash: 'sha256',
};
const EMAIL = TEST_USER.email;
const OLD_PASSWORD = 'old-password-123';
const NEW_PASSWORD = 'new-password-456';

type ServerState = {
  salt: string;
  passwordAuth: string;
  encryptedPrivKey: string;
  pubKey: string;
};

let state: ServerState;
let changeRequests: Array<Record<string, string>>;

async function seedAccount(password: string): Promise<ServerState> {
  const salt = generateRandomKeyBytes(PARAMS.salt_length);
  const { passwordAuth, wrappingKey } = await derivePasswordMaterial(password, salt, PARAMS);
  const keyPair = await generateUserKeyPair();
  return {
    salt: salt.toBase64(),
    passwordAuth: passwordAuth.toBase64(),
    encryptedPrivKey: (await encryptData(wrappingKey, keyPair.privateKey)).toBase64(),
    pubKey: keyPair.publicKey.toBase64(),
  };
}

// A stand-in for the API that keeps the account's key material in memory.
function installAccountHandlers() {
  server.use(
    http.get(`${BASE}/user/login-material`, () =>
      HttpResponse.json({ password_salt: state.salt, params: PARAMS }),
    ),
    http.post(`${BASE}/login`, async ({ request }) => {
      const body = (await request.json()) as { password_auth: string };
      return body.password_auth === state.passwordAuth
        ? new HttpResponse(null, { status: 204 })
        : HttpResponse.json({ error: 'Invalid email or password' }, { status: 401 });
    }),
    http.get(`${BASE}/user`, () =>
      HttpResponse.json({
        ...TEST_USER,
        pub_key: state.pubKey,
        encrypted_priv_key: state.encryptedPrivKey,
      }),
    ),
    http.post(`${BASE}/user/password`, async ({ request }) => {
      const body = (await request.json()) as Record<string, string>;
      changeRequests.push(body);
      if (body.current_password_auth !== state.passwordAuth) {
        return HttpResponse.json({ error: 'Current password is incorrect' }, { status: 403 });
      }
      state = {
        ...state,
        salt: body.password_salt!,
        passwordAuth: body.password_auth!,
        encryptedPrivKey: body.encrypted_priv_key!,
      };
      return new HttpResponse(null, { status: 204 });
    }),
  );
}

beforeEach(async () => {
  localStorage.clear();
  changeRequests = [];
  state = await seedAccount(OLD_PASSWORD);
  installAccountHandlers();
});

describe('Session.changePassword', () => {
  it('keeps the key pair so batches sealed before the change still open after it', async () => {
    const session = await Session.fromLogin(EMAIL, OLD_PASSWORD);
    const originalPubKey = state.pubKey;
    const sealedBefore = await encryptForPublicKey(
      Uint8Array.fromBase64(originalPubKey),
      generateRandomKeyBytes(),
    );

    await session.changePassword(EMAIL, OLD_PASSWORD, NEW_PASSWORD);

    expect(changeRequests).toHaveLength(1);
    expect(changeRequests[0]).not.toHaveProperty('pub_key');
    expect(state.pubKey).toBe(originalPubKey);

    // Logging in fresh with the new password recovers a key that opens the old envelope.
    const relogin = await Session.fromLogin(EMAIL, NEW_PASSWORD);
    expect(relogin.privateKey).not.toBeNull();
    await expect(unwrapBatchKey(relogin.privateKey!, sealedBefore)).resolves.toBeTruthy();

    // The old password no longer works.
    await expect(Session.fromLogin(EMAIL, OLD_PASSWORD)).rejects.toThrow();
  });

  it('persists the new wrapping key so a reload restores the private key', async () => {
    const session = await Session.fromLogin(EMAIL, OLD_PASSWORD);
    await session.changePassword(EMAIL, OLD_PASSWORD, NEW_PASSWORD);

    const restored = await Session.restore();
    expect(restored?.privateKey).not.toBeNull();
  });

  it('rejects a wrong current password before contacting the server', async () => {
    const session = await Session.fromLogin(EMAIL, OLD_PASSWORD);
    const before = { ...state };

    await expect(session.changePassword(EMAIL, 'not-my-password', NEW_PASSWORD)).rejects.toThrow(
      'Current password is incorrect.',
    );

    expect(changeRequests).toHaveLength(0);
    expect(state).toEqual(before);
  });

  it('refuses to re-wrap a private key that does not match the stored public key', async () => {
    const session = await Session.fromLogin(EMAIL, OLD_PASSWORD);
    state = { ...state, pubKey: (await generateUserKeyPair()).publicKey.toBase64() };

    await expect(session.changePassword(EMAIL, OLD_PASSWORD, NEW_PASSWORD)).rejects.toThrow(
      /doesn't match/,
    );
    expect(changeRequests).toHaveLength(0);
  });
});
