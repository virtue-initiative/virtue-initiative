import { afterEach, describe, expect, it } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '../../mocks/server';
import { TEST_UPDATES, TEST_USER } from '../../mocks/fixtures';
import { Session } from './session';

const BASE = 'http://localhost:8787';
const WRAPPING_KEY_STORAGE = 'virtue_wrapping_key';

async function plantWrappingKey(): Promise<void> {
  const key = await crypto.subtle.generateKey({ name: 'AES-GCM', length: 256 }, true, [
    'encrypt',
    'decrypt',
  ]);
  const raw = await crypto.subtle.exportKey('raw', key);
  localStorage.setItem(WRAPPING_KEY_STORAGE, btoa(String.fromCharCode(...new Uint8Array(raw))));
}

afterEach(() => {
  localStorage.clear();
});

describe('Session.restore', () => {
  it('fetches /updates (not /user) and populates both session.user and session.updates', async () => {
    await plantWrappingKey();
    let updatesCalls = 0;
    let userCalls = 0;
    server.use(
      http.get(`${BASE}/updates`, () => {
        updatesCalls++;
        return HttpResponse.json(TEST_UPDATES);
      }),
      http.get(`${BASE}/user`, () => {
        userCalls++;
        return HttpResponse.json(TEST_USER);
      }),
    );

    const session = await Session.restore();

    expect(session).not.toBeNull();
    expect(session!.user).toEqual(TEST_UPDATES.user);
    expect(session!.updates).toEqual(TEST_UPDATES);
    expect(updatesCalls).toBe(1);
    expect(userCalls).toBe(0);
  });

  it('returns null when no wrapping key is stored', async () => {
    const session = await Session.restore();
    expect(session).toBeNull();
  });
});

describe('Session.fromFinishSignup', () => {
  it('populates both session.user and session.updates', async () => {
    let updatesCalls = 0;
    server.use(
      http.get(`${BASE}/updates`, () => {
        updatesCalls++;
        return HttpResponse.json(TEST_UPDATES);
      }),
    );

    const session = await Session.fromFinishSignup('verify-token', 'New User', 'password123');

    expect(session.user).toEqual(TEST_UPDATES.user);
    expect(session.updates).toEqual(TEST_UPDATES);
    expect(updatesCalls).toBe(1);
  }, 20000);
});
