import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createCacheClient, type CacheQueryUpdate } from './client';

// Each createCacheClient() stands in for one browser tab. The tabs share a fake Web Locks
// manager and a fake BroadcastChannel bus, and each gets its own `document`, so a test can
// freeze one tab (fire `freeze` on its document, or make its channel deaf) without the others.

type LockRequest = {
  callback: (lock: { name: string }) => Promise<unknown>;
  resolve: (v: unknown) => void;
  reject: (e: unknown) => void;
};

class FakeLocks {
  holder: LockRequest | null = null;
  queue: LockRequest[] = [];

  request(
    name: string,
    options: { steal?: boolean; signal?: AbortSignal },
    callback: LockRequest['callback'],
  ): Promise<unknown> {
    return new Promise((resolve, reject) => {
      const req: LockRequest = { callback, resolve, reject };
      if (options.steal) {
        const old = this.holder;
        this.holder = null;
        old?.reject(new DOMException('lock stolen', 'AbortError'));
        this.grant(req, name);
        return;
      }
      options.signal?.addEventListener('abort', () => {
        const idx = this.queue.indexOf(req);
        if (idx < 0) return;
        this.queue.splice(idx, 1);
        reject(new DOMException('request aborted', 'AbortError'));
      });
      if (this.holder) this.queue.push(req);
      else this.grant(req, name);
    });
  }

  async query() {
    return { held: this.holder ? [{ name: 'cache-leader' }] : [], pending: [] };
  }

  private grant(req: LockRequest, name: string) {
    this.holder = req;
    // Real Web Locks always run the callback asynchronously.
    void Promise.resolve()
      .then(() => req.callback({ name }))
      .then((v) => {
        // A stolen lock's callback can still finish later; it no longer owns anything.
        if (this.holder !== req) return;
        this.holder = null;
        req.resolve(v);
        const next = this.queue.shift();
        if (next) this.grant(next, name);
      });
  }
}

class FakeBroadcastChannel {
  static all: FakeBroadcastChannel[] = [];
  onmessage: ((e: MessageEvent) => void) | null = null;
  // A deaf channel stands in for a frozen tab that never runs its message handlers.
  deaf = false;

  constructor(readonly name: string) {
    FakeBroadcastChannel.all.push(this);
  }

  postMessage(data: unknown) {
    for (const other of FakeBroadcastChannel.all) {
      if (other === this || other.name !== this.name) continue;
      setTimeout(() => {
        if (!other.deaf) other.onmessage?.({ data } as MessageEvent);
      }, 0);
    }
  }

  close() {}
}

class FakeWorker {
  static all: FakeWorker[] = [];
  onmessage: ((e: MessageEvent) => void) | null = null;
  onerror: ((e: ErrorEvent) => void) | null = null;
  posted: Array<Record<string, unknown>> = [];
  terminated = false;

  constructor() {
    FakeWorker.all.push(this);
  }

  postMessage(msg: Record<string, unknown>) {
    this.posted.push(msg);
  }

  terminate() {
    this.terminated = true;
  }

  reply(data: unknown) {
    this.onmessage?.({ data } as MessageEvent);
  }
}

let locks: FakeLocks;

function openTab() {
  const doc = new EventTarget();
  vi.stubGlobal('document', doc);
  const client = createCacheClient();
  const channel = FakeBroadcastChannel.all[FakeBroadcastChannel.all.length - 1];
  return { client, doc, channel };
}

const liveWorkers = () => FakeWorker.all.filter((w) => !w.terminated);

beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(console, 'log').mockImplementation(() => {});
  vi.spyOn(console, 'warn').mockImplementation(() => {});
  locks = new FakeLocks();
  FakeBroadcastChannel.all = [];
  FakeWorker.all = [];
  vi.stubGlobal('BroadcastChannel', FakeBroadcastChannel);
  vi.stubGlobal('Worker', FakeWorker);
  Object.defineProperty(navigator, 'locks', { value: locks, configurable: true });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('cache client leader handoff', () => {
  it('hands the cache to a waiting tab when the leader freezes', async () => {
    const a = openTab();
    a.client.setSession('user-1', null);
    await vi.advanceTimersByTimeAsync(0);
    const workerA = FakeWorker.all[0];
    expect(workerA).toBeDefined();

    const b = openTab();
    b.client.setSession('user-1', null);
    const updates: CacheQueryUpdate[] = [];
    b.client.cacheQuery({ userId: 'user-1' }, (u) => updates.push(u));
    await vi.advanceTimersByTimeAsync(1000);
    const query = workerA.posted.find((m) => m.method === 'cacheQuery');
    expect(query).toBeDefined();

    a.doc.dispatchEvent(new Event('freeze'));
    await vi.advanceTimersByTimeAsync(1000);

    expect(workerA.terminated).toBe(true);
    expect(liveWorkers()).toHaveLength(1);
    const workerB = liveWorkers()[0];
    // The new worker gets a session before the query it re-serves.
    const methods = workerB.posted.map((m) => m.method);
    expect(methods[0]).toBe('setSession');
    expect(workerB.posted.some((m) => m.method === 'cacheQuery' && m.id === query!.id)).toBe(true);

    workerB.reply({
      type: 'queryChunk',
      id: query!.id,
      logs: [],
      done: true,
      processed: 0,
      total: 0,
      mode: 'replace',
    });
    expect(updates[updates.length - 1]?.done).toBe(true);

    // The frozen tab rejoins as a follower rather than starting a second worker.
    a.doc.dispatchEvent(new Event('resume'));
    await vi.advanceTimersByTimeAsync(1000);
    expect(liveWorkers()).toEqual([workerB]);
  });

  it('steals the lock from a leader that stopped answering', async () => {
    const a = openTab();
    a.client.setSession('user-1', null);
    await vi.advanceTimersByTimeAsync(0);
    const workerA = FakeWorker.all[0];

    const b = openTab();
    b.client.setSession('user-1', null);
    await vi.advanceTimersByTimeAsync(1000);

    // Frozen without a `freeze` event: tab A still holds the lock but hears nothing.
    a.channel.deaf = true;
    const updates: CacheQueryUpdate[] = [];
    b.client.cacheQuery({ userId: 'user-1' }, (u) => updates.push(u));
    await vi.advanceTimersByTimeAsync(3000);
    expect(liveWorkers()).toEqual([workerA]);

    await vi.advanceTimersByTimeAsync(4000);
    // A's lock request rejected with AbortError, so it stood down and terminated its worker.
    expect(workerA.terminated).toBe(true);
    expect(liveWorkers()).toHaveLength(1);
    const workerB = liveWorkers()[0];
    expect(workerB.posted.map((m) => m.method)).toEqual(['setSession', 'cacheQuery']);
  });

  it('settles a query with an error when nothing ever answers it', async () => {
    const a = openTab();
    const updates: CacheQueryUpdate[] = [];
    a.client.cacheQuery({ userId: 'user-1' }, (u) => updates.push(u));
    await vi.advanceTimersByTimeAsync(0);
    expect(FakeWorker.all[0].posted.some((m) => m.method === 'cacheQuery')).toBe(true);

    await vi.advanceTimersByTimeAsync(44_000);
    expect(updates).toEqual([]);

    await vi.advanceTimersByTimeAsync(2000);
    expect(updates).toHaveLength(1);
    expect(updates[0]).toMatchObject({ done: true, error: { kind: 'failed' } });
  });

  it('passes a sync error from the worker through to the query', async () => {
    const a = openTab();
    const updates: CacheQueryUpdate[] = [];
    a.client.cacheQuery({ userId: 'user-1' }, (u) => updates.push(u));
    await vi.advanceTimersByTimeAsync(0);
    const worker = FakeWorker.all[0];
    const query = worker.posted.find((m) => m.method === 'cacheQuery')!;

    worker.reply({
      type: 'queryChunk',
      id: query.id,
      logs: [],
      done: true,
      processed: 0,
      total: 0,
      mode: 'replace',
      error: { kind: 'timeout', message: 'signal timed out' },
    });
    expect(updates).toEqual([
      {
        logs: [],
        replace: true,
        done: true,
        processed: 0,
        total: 0,
        error: { kind: 'timeout', message: 'signal timed out' },
      },
    ]);

    // The watchdog was cleared with the stream, so nothing fires later.
    await vi.advanceTimersByTimeAsync(60_000);
    expect(updates).toHaveLength(1);
  });
});

describe('cache client liveness check', () => {
  it("doesn't steal from a live leader after the follower sat idle", async () => {
    const a = openTab();
    a.client.setSession('user-1', null);
    await vi.advanceTimersByTimeAsync(0);
    const workerA = FakeWorker.all[0];

    const b = openTab();
    b.client.setSession('user-1', null);
    await vi.advanceTimersByTimeAsync(1000);

    // Nothing in flight for a long while, so B hears nothing from A.
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    b.client.cacheQuery({ userId: 'user-1' }, () => {});
    await vi.advanceTimersByTimeAsync(10_000);

    expect(liveWorkers()).toEqual([workerA]);
    expect(workerA.posted.filter((m) => m.method === 'cacheQuery')).toHaveLength(1);
  });
});
