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
      token: string;
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
  setSession(token: string, userId: string, privateKey: CryptoKey | null): void;
  cacheQuery(query: CacheQuery, callback: CacheQueryCallback): void;
  refetch(): void;
  clearCache(): Promise<void>;
  deleteDeviceData(viewerId: string, deviceId: string): Promise<void>;
  getEventImage(eventId: string): Promise<Uint8Array | null>;
  getDeviceBatchEndTimes(
    viewerId: string,
    targetUserId: string,
    deviceId: string,
  ): Promise<number[]>;
}

const CHANNEL_NAME = 'cache-worker';

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

  // Held so HMR disposal can release the lock.
  let lockReleaser: (() => void) | null = null;

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
      if (e.data?.type === 'follower-ping') {
        // A late-opening follower is asking us to re-announce.
        channel.postMessage({ type: 'leader-ready' });
        return;
      }
      if (e.data?.id && leaderWorker) leaderWorker.postMessage(e.data);
    };
  }

  if (!navigator.locks) {
    console.warn('[cache-client] navigator.locks unavailable, acting as leader immediately');
    becomeLeader();
  } else {
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

  // Follower path: responses are broadcast by the leader.
  channel.onmessage = (e: MessageEvent<unknown>) => {
    const data = e.data as Record<string, unknown>;
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
  };

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
    setSession: (token, userId, privateKey) => {
      localUserId = userId;
      send({ id: makeId(), method: 'setSession', token, userId, privateKey });
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

    clearCache: () => call<void>({ method: 'clearCache' }),

    deleteDeviceData: (viewerId, deviceId) =>
      call<void>({ method: 'deleteDeviceData', viewerId, deviceId }),

    getEventImage: (eventId) => call<Uint8Array | null>({ method: 'getEventImage', eventId }),

    getDeviceBatchEndTimes: (viewerId, targetUserId, deviceId) =>
      call<number[]>({ method: 'getDeviceBatchEndTimes', viewerId, targetUserId, deviceId }),
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
