import type { FeedLog } from '../../pages/Logs/types';

export type CacheQuery = {
  userId?: string;
  deviceId?: string;
  startTime?: number;
  endTime?: number;
  eventTypes?: string[];
};

export type CacheRequest =
  | {
      id: string;
      method: 'setSession';
      userId: string;
      privateKey: CryptoKey | null;
    }
  | { id: string; method: 'cacheQuery'; query: CacheQuery; targetUserId: string }
  | { id: string; method: 'refetch' }
  | { id: string; method: 'clearCache' }
  | { id: string; method: 'deleteDeviceData'; viewerId: string; deviceId: string }
  | { id: string; method: 'getEventImage'; eventId: string }
  | {
      id: string;
      method: 'getDeviceBatchEndTimes';
      viewerId: string;
      targetUserId: string;
      deviceId: string;
    }
  | {
      id: string;
      method: 'getDecryptionStats';
      viewerId: string;
      targetUserId: string;
      deviceId?: string;
      startTime?: number;
      endTime?: number;
    };

export type DecryptionStats = {
  totalBatches: number;
  decryptedBatches: number;
  failedBatches: number;
  failureReasons: { error: string; count: number }[];
  totalEvents: number;
  totalScreenshots: number;
};

export type CacheResponse = { id: string; result: unknown } | { id: string; error: string };

export type CacheChunk = {
  type: 'queryChunk';
  id: string;
  logs: FeedLog[];
  done: boolean;
  processed: number;
  total: number;
  mode: 'replace' | 'append';
};

// Counts-only progress signal emitted during a sync; carries no log payload.
export type CacheProgress = { type: 'queryProgress'; id: string; processed: number; total: number };

// A single update delivered to a cacheQuery subscriber. `logs` is present on data updates and
// omitted on the lightweight intermediate progress ticks, where only the block counts change.
// When logs are present, `replace` distinguishes an authoritative snapshot (cached fast-path,
// final result) from an incremental delta the consumer should merge into its existing set.
export type CacheQueryUpdate = {
  logs?: FeedLog[];
  replace?: boolean;
  done: boolean;
  processed: number;
  total: number;
};

export type CacheQueryCallback = (update: CacheQueryUpdate) => void;

export interface CacheClient {
  setSession(userId: string, privateKey: CryptoKey | null): void;
  cacheQuery(query: CacheQuery, callback: CacheQueryCallback): void;
  refetch(): void;
  // Empty the cached data for this tab, e.g. on logout. Leaves the worker running.
  clearCache(): Promise<void>;
  // Full reset: every tab drops its worker and leader lock, the OPFS files are deleted, and
  // the other tabs reload. The caller should reload this tab. Use this for the user-facing
  // "Clear cache" button, since it also recovers from a wedged or stale worker.
  resetCache(): Promise<void>;
  deleteDeviceData(viewerId: string, deviceId: string): Promise<void>;
  getEventImage(eventId: string): Promise<Uint8Array | null>;
  getDeviceBatchEndTimes(
    viewerId: string,
    targetUserId: string,
    deviceId: string,
  ): Promise<number[]>;
  getDecryptionStats(
    viewerId: string,
    targetUserId: string,
    deviceId?: string,
    startTime?: number,
    endTime?: number,
  ): Promise<DecryptionStats>;
}

const CHANNEL_NAME = 'cache-worker';

// How long to wait for the worker to confirm a data wipe before wiping OPFS from this
// thread instead.
const CLEAR_CACHE_TIMEOUT_MS = 8000;

// Directory the SQLite SAH pool VFS keeps its backing files in ("." + the default vfsName).
const SAH_POOL_DIR = '.opfs-sahpool';

// Grace period between telling the other tabs to stand down and deleting the OPFS
// directory, so their workers are gone before we remove the files they had open.
const TEARDOWN_GRACE_MS = 300;

// A tab that stood down but never heard the reset finish reloads anyway, so a tab can't be
// left permanently without a cache because the tab that started the reset went away.
const RESET_FALLBACK_RELOAD_MS = 5000;

// Terminating a worker releases its sync access handles asynchronously, so a removeEntry
// racing that loses to NoModificationAllowedError. Retry briefly rather than give up.
const WIPE_ATTEMPTS = 5;
const WIPE_RETRY_MS = 200;

const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

// Delete the SAH pool directory outright rather than emptying the database through SQLite.
// Every caller terminates the worker first, so this also discards a pool left inconsistent
// by that termination. Requires that no worker in any tab still holds the files open.
async function wipeCacheStorage(): Promise<void> {
  const root = await navigator.storage.getDirectory();
  for (let attempt = 1; ; attempt++) {
    try {
      await root.removeEntry(SAH_POOL_DIR, { recursive: true });
      return;
    } catch (err) {
      if ((err as DOMException).name === 'NotFoundError') return;
      if (attempt >= WIPE_ATTEMPTS) throw err;
      await delay(WIPE_RETRY_MS);
    }
  }
}

type PendingEntry = { resolve: (v: unknown) => void; reject: (e: Error) => void };

// Distributive Omit preserves discriminated union members
type DistOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;
type CacheRequestBody = DistOmit<CacheRequest, 'id'>;

function makeId() {
  return crypto.randomUUID();
}

function handleResponse(msg: CacheResponse, pending: Map<string, PendingEntry>) {
  const entry = pending.get(msg.id);
  if (!entry) return;
  pending.delete(msg.id);
  if ('error' in msg) entry.reject(new Error(msg.error));
  else entry.resolve(msg.result);
}

export function createCacheClient(): CacheClient {
  const pending = new Map<string, PendingEntry>();
  const streamCallbacks = new Map<string, CacheQueryCallback>();
  const channel = new BroadcastChannel(CHANNEL_NAME);
  let leaderWorker: Worker | null = null;
  let localUserId: string | null = null;

  // 'unknown' until we acquire the leader lock or receive leader-ready from the leader.
  type Role = 'unknown' | 'leader' | 'follower';
  let role: Role = 'unknown';

  // Messages sent before role is known are buffered here.
  const sendQueue: CacheRequest[] = [];

  // Held so HMR disposal (and a cache reset) can release the lock.
  let lockReleaser: (() => void) | null = null;

  // Set once a cache reset is under way anywhere. Every tab stops talking to the cache from
  // that point on, because each one is about to reload.
  let resetting = false;

  // Drop the worker and the leader lock. The worker is terminated rather than messaged: a
  // reset has to work when it is wedged, and terminating is also the only way to be rid of
  // a worker running an older build in a tab that has been open across a deploy.
  function standDown() {
    resetting = true;
    leaderWorker?.terminate();
    leaderWorker = null;
    lockReleaser?.();
    lockReleaser = null;
    role = 'unknown';
    sendQueue.length = 0;
    streamCallbacks.clear();
    for (const entry of pending.values()) entry.reject(new Error('cache reset'));
    pending.clear();
    // becomeLeader() replaced this handler; we are no longer the leader.
    channel.onmessage = followerChannelHandler;
  }

  // Come back to life after a wipe that isn't followed by a reload, so a logged-out tab
  // that logs back in without reloading still gets a working cache.
  function rearm() {
    resetting = false;
    pingScheduled = false;
    requestLeadership();
  }

  // Control messages both the leader and the follower channel handlers must honour.
  // Returns true when the message was a control message and needs no further handling.
  function handleResetControl(data: Record<string, unknown> | undefined): boolean {
    if (data?.type === 'cache-reset') {
      // Another tab is wiping the cache. Let go of the worker and the lock so its wipe
      // isn't blocked by our open files, then wait to be told the wipe is done.
      console.log('[cache-client] cache reset started elsewhere, standing down');
      standDown();
      setTimeout(() => window.location.reload(), RESET_FALLBACK_RELOAD_MS);
      return true;
    }
    if (data?.type === 'cache-reset-done') {
      console.log('[cache-client] cache reset finished elsewhere, reloading');
      window.location.reload();
      return true;
    }
    return false;
  }

  // Send a follower-ping once if we're still unknown after a short delay,
  // so tabs that open after leader-ready was already broadcast can discover the leader.
  let pingScheduled = false;
  function schedulePing() {
    if (pingScheduled) return;
    pingScheduled = true;
    setTimeout(() => {
      if (role === 'unknown') {
        console.log('[cache-client] no leader yet, broadcasting follower-ping');
        channel.postMessage({ type: 'follower-ping' });
      }
    }, 100);
  }

  function send(req: CacheRequest) {
    if (resetting) {
      console.log('[cache-client] dropping', req.method, '— cache reset in progress');
      return;
    }
    if (role === 'leader' && leaderWorker) {
      console.log('[cache-client] → worker', req.method);
      leaderWorker.postMessage(req);
    } else if (role === 'follower') {
      console.log('[cache-client] → channel (follower)', req.method);
      channel.postMessage(req);
    } else {
      console.log('[cache-client] queued (no leader yet)', req.method);
      sendQueue.push(req);
      schedulePing();
    }
  }

  function handleChunk(data: CacheChunk) {
    const cb = streamCallbacks.get(data.id);
    if (!cb) return;
    cb({
      logs: data.logs,
      replace: data.mode === 'replace',
      done: data.done,
      processed: data.processed,
      total: data.total,
    });
    if (data.done) streamCallbacks.delete(data.id);
  }

  function handleProgress(data: CacheProgress) {
    const cb = streamCallbacks.get(data.id);
    if (!cb) return;
    cb({ done: false, processed: data.processed, total: data.total });
  }

  function becomeLeader() {
    // The lock can be handed to us by a tab that stood down for a reset. Don't start a
    // worker that would reopen the database the reset is in the middle of deleting.
    if (resetting) {
      console.log('[cache-client] got leader lock during a cache reset, not starting a worker');
      return;
    }
    console.log('[cache-client] acquired leader lock, starting worker');
    role = 'leader';
    // The `new URL(..., import.meta.url)` must be inline here — Vite only
    // statically detects and bundles the worker when it's the direct argument
    // to `new Worker(...)`. Hoisting it to a variable makes Vite skip bundling
    // and emit the raw .ts file, which prod serves as video/mp2t and the
    // browser refuses to execute.
    leaderWorker = new Worker(new URL('./worker.ts', import.meta.url), {
      type: 'module',
    });

    leaderWorker.onerror = (e) => {
      console.error('[cache-client] worker error', e.message, e);
    };

    // Flush any messages buffered before we knew we were the leader.
    const queued = sendQueue.splice(0);
    console.log('[cache-client] flushing', queued.length, 'queued messages to worker');
    for (const msg of queued) {
      console.log('[cache-client] → worker (flushed)', msg.method);
      leaderWorker.postMessage(msg);
    }

    leaderWorker.onmessage = (e: MessageEvent) => {
      const data = e.data as CacheResponse | CacheChunk | CacheProgress;
      if ('type' in data && data.type === 'queryChunk') {
        handleChunk(data as CacheChunk);
        channel.postMessage(data);
      } else if ('type' in data && data.type === 'queryProgress') {
        handleProgress(data as CacheProgress);
        channel.postMessage(data);
      } else {
        handleResponse(data as CacheResponse, pending);
        channel.postMessage(data);
      }
    };

    channel.postMessage({ type: 'leader-ready' });

    channel.onmessage = (e: MessageEvent) => {
      if (handleResetControl(e.data)) return;
      if (e.data?.type === 'follower-ping') {
        // A late-opening follower is asking us to re-announce.
        channel.postMessage({ type: 'leader-ready' });
        return;
      }
      if (e.data?.id && leaderWorker) leaderWorker.postMessage(e.data);
    };
  }

  function requestLeadership() {
    if (!navigator.locks) {
      console.warn('[cache-client] navigator.locks unavailable, acting as leader immediately');
      becomeLeader();
      return;
    }
    // The first tab to acquire this lock becomes the leader and owns the DedicatedWorker.
    // When the leader tab closes, the next-queued tab automatically becomes the new leader.
    navigator.locks
      .request('cache-leader', async () => {
        becomeLeader();
        await new Promise<void>((resolve) => {
          lockReleaser = resolve;
        });
      })
      .catch((err) => console.error('[cache-client] lock request failed', err));
  }

  requestLeadership();

  // Follower path: responses are broadcast by the leader. Named so standDown() can restore
  // it: becomeLeader() replaces channel.onmessage, and a tab that gave up leadership has to
  // go back to reading the leader's broadcasts.
  function followerChannelHandler(e: MessageEvent<unknown>) {
    const data = e.data as Record<string, unknown>;
    if (handleResetControl(data)) return;
    if (data?.type === 'leader-ready') {
      if (role !== 'unknown') return;
      role = 'follower';
      // Flush buffered messages through the channel now that the leader is ready.
      const queued = sendQueue.splice(0);
      console.log(
        '[cache-client] follower: leader-ready, flushing',
        queued.length,
        'queued messages',
      );
      for (const msg of queued) {
        channel.postMessage(msg);
      }
      return;
    }
    if (data?.type === 'queryChunk') {
      handleChunk(data as unknown as CacheChunk);
      return;
    }
    if (data?.type === 'queryProgress') {
      handleProgress(data as unknown as CacheProgress);
      return;
    }
    handleResponse(data as unknown as CacheResponse, pending);
  }

  channel.onmessage = followerChannelHandler;

  function call<T>(req: CacheRequestBody): Promise<T> {
    const id = makeId();
    return new Promise<T>((resolve, reject) => {
      pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      send({ ...req, id } as CacheRequest);
    });
  }

  // Release the leader lock and stop the worker on HMR module replacement,
  // so the next module instance can acquire the lock immediately.
  if (import.meta.hot) {
    import.meta.hot.dispose(() => {
      lockReleaser?.();
      leaderWorker?.terminate();
      channel.close();
    });
  }

  return {
    setSession: (userId, privateKey) => {
      localUserId = userId;
      send({ id: makeId(), method: 'setSession', userId, privateKey });
    },

    cacheQuery: (query, callback) => {
      const id = makeId();
      streamCallbacks.set(id, callback);
      const targetUserId = query.userId ?? localUserId ?? '';
      send({ id, method: 'cacheQuery', query, targetUserId });
    },

    refetch: () => {
      send({ id: makeId(), method: 'refetch' });
    },

    // Data wipe only: hand it to the worker, which empties the pool files and reopens.
    // Falls back to a forceful local wipe if the worker doesn't answer, then re-arms, since
    // callers (logout, session invalidation) keep using this tab afterwards.
    clearCache: async () => {
      try {
        await Promise.race([
          call<void>({ method: 'clearCache' }),
          new Promise<never>((_, reject) =>
            setTimeout(() => reject(new Error('clearCache timed out')), CLEAR_CACHE_TIMEOUT_MS),
          ),
        ]);
        return;
      } catch (err) {
        console.warn('[cache-client] clearCache via worker failed, wiping OPFS directly', err);
      }
      standDown();
      try {
        await wipeCacheStorage();
      } catch (err) {
        console.warn('[cache-client] direct OPFS wipe failed', err);
      }
      rearm();
    },

    // Full reset. Terminating rather than messaging is what makes this work when the worker
    // is wedged, and reloading every tab is what clears a worker still running an older
    // build in a tab that has been open across a deploy. Always resolves, so a failed wipe
    // can't leave the UI stuck; the caller reloads this tab afterwards.
    resetCache: async () => {
      console.log('[cache-client] cache reset: standing down every tab');
      channel.postMessage({ type: 'cache-reset' });
      standDown();
      // Give the other tabs a moment to terminate their own workers before deleting files
      // those workers may still hold open.
      await delay(TEARDOWN_GRACE_MS);
      try {
        await wipeCacheStorage();
        console.log('[cache-client] cache reset: OPFS wiped');
      } catch (err) {
        // The reload still gets every tab a fresh worker, and that worker's schema check
        // rebuilds a drifted database, so this is worth reporting but not worth blocking on.
        console.warn('[cache-client] cache reset: OPFS wipe failed', err);
      }
      channel.postMessage({ type: 'cache-reset-done' });
    },

    deleteDeviceData: (viewerId, deviceId) =>
      call<void>({ method: 'deleteDeviceData', viewerId, deviceId }),

    getEventImage: (eventId) => call<Uint8Array | null>({ method: 'getEventImage', eventId }),

    getDeviceBatchEndTimes: (viewerId, targetUserId, deviceId) =>
      call<number[]>({ method: 'getDeviceBatchEndTimes', viewerId, targetUserId, deviceId }),

    getDecryptionStats: (viewerId, targetUserId, deviceId, startTime, endTime) =>
      call<DecryptionStats>({
        method: 'getDecryptionStats',
        viewerId,
        targetUserId,
        deviceId,
        startTime,
        endTime,
      }),
  };
}

// Requires a DOM window plus Web Worker + BroadcastChannel support. Environments
// without them (SSR, the jsdom/happy-dom test runner) fall back to a null client;
// callers treat cacheQuery as a no-op there.
const cacheClientSupported =
  typeof window !== 'undefined' &&
  typeof Worker !== 'undefined' &&
  typeof BroadcastChannel !== 'undefined';

export const cacheClient: CacheClient | null = cacheClientSupported ? createCacheClient() : null;

export function logCacheQuery(query: CacheQuery, callback: CacheQueryCallback): void {
  cacheClient?.cacheQuery(query, callback);
}

export function triggerRefetch(): void {
  cacheClient?.refetch();
}

if (typeof window !== 'undefined') {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (window as any).createCacheClient = createCacheClient;
}
