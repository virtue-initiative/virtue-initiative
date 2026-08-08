import { describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { APIClient } from './client';
import { server } from '../../mocks/server';
import { TEST_DEVICES, TEST_USER, TEST_WATCHER, TEST_WATCHING } from '../../mocks/fixtures';
import { makeFakeSession } from '../../test-utils';

const BASE = 'http://localhost:8787';

function makeClient() {
  return new APIClient(makeFakeSession());
}

describe('APIClient — user cache', () => {
  it('fetches and caches the user on first getUser()', async () => {
    const client = makeClient();
    client.getUser(); // triggers async fetch
    await vi.waitFor(() => expect(client.getUser()).toEqual(TEST_USER));
  });

  it('notifies user subscribers when user loads', async () => {
    const client = makeClient();
    const cb = vi.fn();
    client.subscribeUser(cb);
    await vi.waitFor(() => expect(cb).toHaveBeenCalledWith(TEST_USER));
  });

  it('deduplicates in-flight user fetches', async () => {
    let callCount = 0;
    server.use(
      http.get(`${BASE}/user`, () => {
        callCount++;
        return HttpResponse.json(TEST_USER);
      }),
    );
    const client = makeClient();
    client.getUser();
    client.getUser();
    client.getUser();
    await vi.waitFor(() => expect(client.getUser()).toEqual(TEST_USER));
    expect(callCount).toBe(1);
  });

  it('unsubscribes correctly', async () => {
    const client = makeClient();
    const cb = vi.fn();
    const { unsubscribe } = client.subscribeUser(cb);
    unsubscribe();
    await vi.waitFor(() => expect(client.getUser()).toEqual(TEST_USER));
    expect(cb).not.toHaveBeenCalled();
  });

  it('seeds the cache from session.user and skips the fetch', async () => {
    let callCount = 0;
    server.use(
      http.get(`${BASE}/user`, () => {
        callCount++;
        return HttpResponse.json(TEST_USER);
      }),
    );
    const client = new APIClient(makeFakeSession({ user: TEST_USER }));
    expect(client.getUser()).toEqual(TEST_USER);
    expect(callCount).toBe(0);
  });
});

describe('APIClient — devices cache', () => {
  it('fetches devices and notifies subscribers', async () => {
    const client = makeClient();
    const cb = vi.fn();
    client.subscribeDevices(cb);
    await vi.waitFor(() => expect(cb).toHaveBeenCalledWith(TEST_DEVICES));
  });

  it('returns initial empty array before fetch completes', () => {
    const client = makeClient();
    expect(client.listDevices()).toEqual([]);
  });

  it('deduplicates in-flight device fetches', async () => {
    let callCount = 0;
    server.use(
      http.get(`${BASE}/device`, () => {
        callCount++;
        return HttpResponse.json(TEST_DEVICES);
      }),
    );
    const client = makeClient();
    client.listDevices();
    client.listDevices();
    await vi.waitFor(() => expect(client.listDevices()).toHaveLength(TEST_DEVICES.length));
    expect(callCount).toBe(1);
  });
});

describe('APIClient — partners cache', () => {
  it('fetches watchers and watchings', async () => {
    const client = makeClient();
    const watcherCb = vi.fn();
    const watchingCb = vi.fn();
    client.subscribeWatchers(watcherCb);
    client.subscribeWatchings(watchingCb);
    await vi.waitFor(() => {
      expect(watcherCb).toHaveBeenCalledWith([TEST_WATCHER]);
      expect(watchingCb).toHaveBeenCalledWith([TEST_WATCHING]);
    });
  });

  it('deduplicates in-flight partner fetches', async () => {
    let callCount = 0;
    server.use(
      http.get(`${BASE}/partner`, () => {
        callCount++;
        return HttpResponse.json({ watchers: [TEST_WATCHER], watching: [TEST_WATCHING] });
      }),
    );
    const client = makeClient();
    client.listWatchers();
    client.listWatchings();
    await vi.waitFor(() => expect(client.listWatchers()).toHaveLength(1));
    expect(callCount).toBe(1);
  });
});

describe('APIClient — logout', () => {
  it('fires onLogout subscribers when token refresh fails', async () => {
    const session = makeFakeSession();
    let tokenRefreshFailedCb: (() => void) | null = null;
    (session.onTokenRefreshFailed as ReturnType<typeof vi.fn>).mockImplementation(
      (cb: () => void) => {
        tokenRefreshFailedCb = cb;
      },
    );
    const client = new APIClient(session);
    const logoutCb = vi.fn();
    client.onLogout(logoutCb);
    // Trigger token refresh failure
    tokenRefreshFailedCb?.();
    expect(logoutCb).toHaveBeenCalledTimes(1);
  });

  it('fires logout only once even if token refresh fails multiple times', async () => {
    const session = makeFakeSession();
    let tokenRefreshFailedCb: (() => void) | null = null;
    (session.onTokenRefreshFailed as ReturnType<typeof vi.fn>).mockImplementation(
      (cb: () => void) => {
        tokenRefreshFailedCb = cb;
      },
    );
    const client = new APIClient(session);
    const logoutCb = vi.fn();
    client.onLogout(logoutCb);
    tokenRefreshFailedCb?.();
    tokenRefreshFailedCb?.();
    expect(logoutCb).toHaveBeenCalledTimes(1);
  });

  it('updateSettings calls PATCH /user and refreshes cache', async () => {
    let patchCalled = false;
    server.use(
      http.patch(`${BASE}/user`, async () => {
        patchCalled = true;
        return HttpResponse.json({ email_verification_required: false });
      }),
    );
    const client = makeClient();
    await client.updateSettings({ name: 'New Name' });
    expect(patchCalled).toBe(true);
  });
});

describe('APIClient — updateDevice / removeDevice', () => {
  it('calls PATCH /device/:id', async () => {
    let patchedId: string | undefined;
    server.use(
      http.patch(`${BASE}/device/:id`, ({ params }) => {
        patchedId = params.id as string;
        return HttpResponse.json({ ...TEST_DEVICES[0], name: 'New Name' });
      }),
    );
    const client = makeClient();
    await client.updateDevice('device-1', { name: 'New Name' });
    expect(patchedId).toBe('device-1');
  });

  it('calls DELETE /device/:id', async () => {
    let deletedId: string | undefined;
    server.use(
      http.delete(`${BASE}/device/:id`, ({ params }) => {
        deletedId = params.id as string;
        return new HttpResponse(null, { status: 204 });
      }),
    );
    const client = makeClient();
    await client.removeDevice('device-1');
    expect(deletedId).toBe('device-1');
  });
});

describe('APIClient — invitePartner / removeWatcher / stopWatching', () => {
  it('calls POST /partner for invitePartner', async () => {
    let body: unknown;
    server.use(
      http.post(`${BASE}/partner`, async ({ request }) => {
        body = await request.json();
        return HttpResponse.json({ id: 'new-1', invite_token: 'tok' });
      }),
    );
    const client = makeClient();
    await client.invitePartner('alice@example.com');
    expect((body as { email: string }).email).toBe('alice@example.com');
  });

  it('calls DELETE /partner/watcher/:id for removeWatcher', async () => {
    let deletedId: string | undefined;
    server.use(
      http.delete(`${BASE}/partner/watcher/:id`, ({ params }) => {
        deletedId = params.id as string;
        return new HttpResponse(null, { status: 204 });
      }),
    );
    const client = makeClient();
    await client.removeWatcher('watcher-1');
    expect(deletedId).toBe('watcher-1');
  });

  it('calls DELETE /partner/watching/:id for stopWatching', async () => {
    let deletedId: string | undefined;
    server.use(
      http.delete(`${BASE}/partner/watching/:id`, ({ params }) => {
        deletedId = params.id as string;
        return new HttpResponse(null, { status: 204 });
      }),
    );
    const client = makeClient();
    await client.stopWatching('watching-1');
    expect(deletedId).toBe('watching-1');
  });
});
