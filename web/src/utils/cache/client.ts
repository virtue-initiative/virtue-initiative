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

// Why a query finished without a fresh sync. `network` and `timeout` mean the server couldn't
// be reached, `cache-locked` means another tab still holds the cache database open, and
// `failed` covers everything else.
export type CacheQueryErrorKind = 'network' | 'timeout' | 'cache-locked' | 'failed';
export type CacheQueryError = { kind: CacheQueryErrorKind; message: string };

export type CacheChunk = {
  type: 'queryChunk';
  id: string;
  logs: FeedLog[];
  done: boolean;
  processed: number;
  total: number;
  mode: 'replace' | 'append';
  // Only on a final (`done`) chunk: the sync didn't complete, and `logs` is what's cached.
  error?: CacheQueryError;
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
  error?: CacheQueryError;
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

const LEADER_LOCK = 'cache-leader';

// While a tab has requests waiting on a leader, it pings for one this often.
const LIVENESS_INTERVAL_MS = 500;

// A leader that answers nothing for this long is treated as frozen, and its lock is stolen.
// A frozen tab (Android freezes background tabs) keeps its Web Lock, so without this every
// other tab would wait on it forever. Jittered per tab so two waiting tabs rarely both steal.
const LEADER_SILENT_MS = 4000;
const LEADER_SILENT_JITTER_MS = 1000;

// A query stream that hears nothing for this long is settled with an error. It is a backstop
// for anything else going wrong, so it sits above the worker's own fetch timeouts (30s for
// /data, 20s per batch), which already settle a query whose network requests stall.
const STREAM_STALL_MS = 45_000;

type PendingEntry = {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  // Kept so the request can be re-sent if the leader changes before it's answered.
  req: CacheRequest;
};

type OpenStream = {
  callback: CacheQueryCallback;
  req: CacheRequest;
  watchdog: ReturnType<typeof setTimeout> | null;
};

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
  const streams = new Map<string, OpenStream>();
  const channel = new BroadcastChannel(CHANNEL_NAME);
  const tabId = makeId();
  let leaderWorker: Worker | null = null;
  let localUserId: string | null = null;
  // Re-sent to whichever worker takes over, since a new worker starts with no session.
  let lastSession: CacheRequestBody | null = null;

  // 'unknown' until we acquire the leader lock or receive leader-ready from the leader.
  type Role = 'unknown' | 'leader' | 'follower';
  let role: Role = 'unknown';
  // The leader this tab follows, from its leader-ready. A different id means a new leader
  // that has never seen this tab's requests.
  let currentLeaderId: string | null = null;
  let lastLeaderSeen = Date.now();
  const leaderSilentLimit = LEADER_SILENT_MS + Math.random() * LEADER_SILENT_JITTER_MS;

  // Messages sent before role is known are buffered here.
  let sendQueue: CacheRequest[] = [];

  // Held so HMR disposal (and a cache reset) can release the lock.
  let lockReleaser: (() => void) | null = null;
  // Withdraws this tab's queued (not yet granted) lock request.
  let lockRequestAbort: AbortController | null = null;

  // Set once a cache reset is under way anywhere. Every tab stops talking to the cache from
  // that point on, because each one is about to reload.
  let resetting = false;

  // Set between the page lifecycle `freeze` and `resume` events. Timers that fire late
  // because the tab was frozen mustn't be mistaken for a silent leader or a stalled query.
  let frozen = false;

  // Drop the worker and the leader lock. The worker is terminated rather than messaged: a
  // reset has to work when it is wedged, and terminating is also the only way to be rid of
  // a worker running an older build in a tab that has been open across a deploy.
  function standDown() {
    resetting = true;
    leaderWorker?.terminate();
    leaderWorker = null;
    lockReleaser?.();
    lockReleaser = null;
    lockRequestAbort?.abort();
    lockRequestAbort = null;
    role = 'unknown';
    currentLeaderId = null;
    stopLivenessCheck();
    sendQueue = [];
    for (const stream of streams.values()) {
      if (stream.watchdog) clearTimeout(stream.watchdog);
    }
    streams.clear();
    for (const entry of pending.values()) entry.reject(new Error('cache reset'));
    pending.clear();
    // becomeLeader() replaced this handler; we are no longer the leader.
    channel.onmessage = followerChannelHandler;
  }

  // Come back to life after a wipe that isn't followed by a reload, so a logged-out tab
  // that logs back in without reloading still gets a working cache.
  function rearm() {
    resetting = false;
    lastLeaderSeen = Date.now();
    requestLeadership();
  }

  // Give up leadership without a reset: on `freeze`, or after another tab stole the lock.
  // Terminating the worker releases its OPFS handles so the next leader's worker can open
  // the database. Anything this tab still has in flight goes to the next leader.
  function relinquishLeadership() {
    if (role !== 'leader') return;
    leaderWorker?.terminate();
    leaderWorker = null;
    const release = lockReleaser;
    lockReleaser = null;
    release?.();
    role = 'unknown';
    currentLeaderId = null;
    lastLeaderSeen = Date.now();
    channel.onmessage = followerChannelHandler;
    resendInFlight();
  }

  // Every unanswered call and every open query stream.
  function inFlightRequests(): CacheRequest[] {
    return [
      ...[...pending.values()].map((entry) => entry.req),
      ...[...streams.values()].map((s) => s.req),
    ];
  }

  // Re-send the session and everything in flight to a leader that hasn't seen them.
  // Duplicates are harmless: a response or final chunk for an id that has already settled
  // is ignored.
  function resendInFlight() {
    const reqs: CacheRequest[] = [];
    if (lastSession) reqs.push({ ...lastSession, id: makeId() } as CacheRequest);
    reqs.push(...inFlightRequests());
    console.log('[cache-client] re-sending', reqs.length, 'in-flight requests to a new leader');
    if (role === 'follower') {
      for (const req of reqs) channel.postMessage(req);
      return;
    }
    const queued = new Set(sendQueue.map((req) => req.id));
    for (const req of reqs) if (!queued.has(req.id)) sendQueue.push(req);
    ensureLivenessCheck();
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

  // While this tab has requests waiting on a leader, ping for one. A late-opening tab finds
  // the current leader this way, and a leader that stops answering (frozen) gets its lock
  // stolen so this tab can take over.
  let livenessTimer: ReturnType<typeof setInterval> | null = null;
  let stealing = false;

  function hasInFlight() {
    return streams.size > 0 || pending.size > 0 || sendQueue.length > 0;
  }

  function ensureLivenessCheck() {
    if (livenessTimer || role === 'leader' || resetting) return;
    // The silence clock starts now. An idle follower may not have heard from the leader in
    // minutes, which says nothing about whether it's alive.
    lastLeaderSeen = Date.now();
    livenessTimer = setInterval(() => void checkLeader(), LIVENESS_INTERVAL_MS);
  }

  function stopLivenessCheck() {
    if (livenessTimer) clearInterval(livenessTimer);
    livenessTimer = null;
  }

  async function checkLeader() {
    if (role === 'leader' || resetting || !hasInFlight()) {
      stopLivenessCheck();
      return;
    }
    if (frozen) return;
    channel.postMessage({ type: 'follower-ping' });
    if (stealing || !navigator.locks || Date.now() - lastLeaderSeen < leaderSilentLimit) return;
    stealing = true;
    try {
      const { held } = await navigator.locks.query();
      // `role` may have changed during the await; TS narrowed it from the check above.
      if (
        (role as Role) === 'leader' ||
        resetting ||
        Date.now() - lastLeaderSeen < leaderSilentLimit
      ) {
        return;
      }
      // Nobody holds the lock, so this tab's own queued request is about to be granted.
      if (!held?.some((lock) => lock.name === LEADER_LOCK)) return;
      console.warn(
        '[cache-client] leader has not answered for',
        Date.now() - lastLeaderSeen,
        'ms, taking over',
      );
      if (role === 'follower') {
        role = 'unknown';
        currentLeaderId = null;
        resendInFlight();
      }
      lastLeaderSeen = Date.now();
      requestLeadership(true);
    } finally {
      stealing = false;
    }
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
      ensureLivenessCheck();
    } else {
      console.log('[cache-client] queued (no leader yet)', req.method);
      sendQueue.push(req);
      ensureLivenessCheck();
    }
  }

  function armWatchdog(id: string) {
    const stream = streams.get(id);
    if (!stream) return;
    if (stream.watchdog) clearTimeout(stream.watchdog);
    stream.watchdog = setTimeout(() => {
      if (frozen) return; // re-armed on resume
      console.warn('[cache-client] query', id, 'stalled, settling it with an error');
      endStream(id)?.({
        done: true,
        processed: 0,
        total: 0,
        error: { kind: 'failed', message: 'The log cache stopped responding.' },
      });
    }, STREAM_STALL_MS);
  }

  // Forget a stream and return its callback, for a final update.
  function endStream(id: string): CacheQueryCallback | undefined {
    const stream = streams.get(id);
    if (!stream) return undefined;
    if (stream.watchdog) clearTimeout(stream.watchdog);
    streams.delete(id);
    return stream.callback;
  }

  function handleChunk(data: CacheChunk) {
    const stream = streams.get(data.id);
    if (!stream) return;
    if (data.done) endStream(data.id);
    else armWatchdog(data.id);
    stream.callback({
      logs: data.logs,
      replace: data.mode === 'replace',
      done: data.done,
      processed: data.processed,
      total: data.total,
      ...(data.error ? { error: data.error } : {}),
    });
  }

  function handleProgress(data: CacheProgress) {
    const stream = streams.get(data.id);
    if (!stream) return;
    armWatchdog(data.id);
    stream.callback({ done: false, processed: data.processed, total: data.total });
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
    currentLeaderId = tabId;
    stopLivenessCheck();
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

    // Flush any messages buffered before we knew we were the leader, plus anything this tab
    // sent to a previous leader that went away unanswered: a follower can be granted the lock
    // before the old leader's leader-leaving reaches it. A fresh worker has no session, so
    // make sure one goes first when this tab has it.
    const queued = sendQueue.splice(0);
    const queuedIds = new Set(queued.map((msg) => msg.id));
    for (const req of inFlightRequests()) if (!queuedIds.has(req.id)) queued.push(req);
    if (lastSession && !queued.some((msg) => msg.method === 'setSession')) {
      queued.unshift({ ...lastSession, id: makeId() } as CacheRequest);
    }
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

    channel.postMessage({ type: 'leader-ready', leaderId: tabId });

    channel.onmessage = (e: MessageEvent) => {
      if (handleResetControl(e.data)) return;
      if (e.data?.type === 'follower-ping') {
        // A late-opening follower is asking us to re-announce, or a follower is checking
        // that we're still alive.
        channel.postMessage({ type: 'leader-ready', leaderId: tabId });
        return;
      }
      if (e.data?.id && leaderWorker) leaderWorker.postMessage(e.data);
    };
  }

  // Queue for the leader lock, or with `steal` take it from a leader that stopped answering.
  function requestLeadership(steal = false) {
    if (!navigator.locks) {
      console.warn('[cache-client] navigator.locks unavailable, acting as leader immediately');
      becomeLeader();
      return;
    }
    // Only one request per tab: a steal replaces the queued one, which would otherwise be
    // granted to us again later.
    lockRequestAbort?.abort();
    const abort = new AbortController();
    lockRequestAbort = steal ? null : abort;
    let granted = false;
    // The first tab to acquire this lock becomes the leader and owns the DedicatedWorker.
    // When the leader tab closes, the next-queued tab automatically becomes the new leader.
    navigator.locks
      .request(LEADER_LOCK, steal ? { steal: true } : { signal: abort.signal }, async () => {
        granted = true;
        if (lockRequestAbort === abort) lockRequestAbort = null;
        becomeLeader();
        await new Promise<void>((resolve) => {
          lockReleaser = resolve;
        });
      })
      .catch((err) => {
        if ((err as DOMException).name !== 'AbortError') {
          console.error('[cache-client] lock request failed', err);
          return;
        }
        // Not granted yet: this tab withdrew the request itself.
        if (!granted) return;
        // Granted, then taken: another tab decided this one was frozen and stole the lock.
        console.warn('[cache-client] another tab took the leader lock, standing down');
        relinquishLeadership();
        if (!resetting && !frozen) requestLeadership();
      });
  }

  // Follower path: responses are broadcast by the leader. Named so standDown() can restore
  // it: becomeLeader() replaces channel.onmessage, and a tab that gave up leadership has to
  // go back to reading the leader's broadcasts.
  function followerChannelHandler(e: MessageEvent<unknown>) {
    const data = e.data as Record<string, unknown>;
    if (handleResetControl(data)) return;
    if (data?.type === 'leader-leaving') {
      // The leader is about to be frozen and has let go of the lock. The next leader has
      // never seen this tab's requests.
      if (role !== 'follower') return;
      console.log('[cache-client] follower: leader leaving, waiting for the next one');
      role = 'unknown';
      currentLeaderId = null;
      lastLeaderSeen = Date.now();
      resendInFlight();
      return;
    }
    if (data?.type === 'leader-ready') {
      lastLeaderSeen = Date.now();
      const leaderId = (data.leaderId as string | undefined) ?? null;
      if (role === 'follower' && leaderId !== currentLeaderId) {
        // A new leader took over without a leader-leaving, e.g. after a steal.
        console.log('[cache-client] follower: new leader', leaderId);
        currentLeaderId = leaderId;
        resendInFlight();
        return;
      }
      if (role !== 'unknown') return;
      role = 'follower';
      currentLeaderId = leaderId;
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
      lastLeaderSeen = Date.now();
      handleChunk(data as unknown as CacheChunk);
      return;
    }
    if (data?.type === 'queryProgress') {
      lastLeaderSeen = Date.now();
      handleProgress(data as unknown as CacheProgress);
      return;
    }
    if (data?.id) lastLeaderSeen = Date.now();
    handleResponse(data as unknown as CacheResponse, pending);
  }

  // Installed before requesting the lock: becomeLeader() replaces it, and without
  // navigator.locks that happens synchronously inside requestLeadership().
  channel.onmessage = followerChannelHandler;
  requestLeadership();

  // Chrome freezes background tabs (on Android, aggressively). A frozen tab keeps its Web
  // Lock, and its worker keeps the OPFS handles, so a frozen leader would stall every other
  // tab. Hand over before freezing instead: terminate the worker, release the lock, and
  // withdraw any queued lock request so the lock isn't granted to a tab that can't run.
  if (typeof document !== 'undefined') {
    document.addEventListener('freeze', () => {
      if (resetting) return;
      console.log('[cache-client] tab freezing, releasing the cache');
      frozen = true;
      stopLivenessCheck();
      lockRequestAbort?.abort();
      lockRequestAbort = null;
      if (role === 'leader') {
        channel.postMessage({ type: 'leader-leaving' });
        relinquishLeadership();
        stopLivenessCheck();
      }
    });
    document.addEventListener('resume', () => {
      if (resetting) return;
      console.log('[cache-client] tab resumed, rejoining the cache');
      frozen = false;
      lastLeaderSeen = Date.now();
      for (const id of streams.keys()) armWatchdog(id);
      requestLeadership();
      ensureLivenessCheck();
    });
  }

  function call<T>(req: CacheRequestBody): Promise<T> {
    const id = makeId();
    const full = { ...req, id } as CacheRequest;
    return new Promise<T>((resolve, reject) => {
      pending.set(id, { resolve: resolve as (v: unknown) => void, reject, req: full });
      send(full);
    });
  }

  // Release the leader lock and stop the worker on HMR module replacement,
  // so the next module instance can acquire the lock immediately.
  if (import.meta.hot) {
    import.meta.hot.dispose(() => {
      lockRequestAbort?.abort();
      lockReleaser?.();
      leaderWorker?.terminate();
      stopLivenessCheck();
      channel.close();
    });
  }

  return {
    setSession: (userId, privateKey) => {
      localUserId = userId;
      lastSession = { method: 'setSession', userId, privateKey };
      send({ id: makeId(), method: 'setSession', userId, privateKey });
    },

    cacheQuery: (query, callback) => {
      const id = makeId();
      const targetUserId = query.userId ?? localUserId ?? '';
      const req: CacheRequest = { id, method: 'cacheQuery', query, targetUserId };
      streams.set(id, { callback, req, watchdog: null });
      armWatchdog(id);
      send(req);
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
