// Drives the real cache worker (worker.ts) through its message protocol, with an in-memory
// SQLite in place of OPFS and MSW serving /data and the batch blobs. The blobs are the
// Rust-made golden vectors in batch-format/vectors/batch.json, opened with the vectors'
// fixed recipient key, so this proves the whole sync -> decrypt -> verify -> store path
// that the Logs page reads from.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { server } from '../../mocks/server';
import { TEST_USER } from '../../mocks/fixtures';
import { importUserPrivateKey } from '../api/crypto';
import type { FeedLog } from '../../pages/Logs/types';

vi.mock('@sqlite.org/sqlite-wasm', async () => {
  const actual =
    await vi.importActual<typeof import('@sqlite.org/sqlite-wasm')>('@sqlite.org/sqlite-wasm');
  return {
    default: async () => {
      const sqlite3 = (await actual.default()) as any; // eslint-disable-line @typescript-eslint/no-explicit-any
      // The browser keeps the cache in OPFS; a test keeps it in memory.
      sqlite3.installOpfsSAHPoolVfs = async () => ({
        OpfsSAHPoolDb: class extends sqlite3.oo1.DB {
          constructor() {
            super(':memory:');
          }
        },
        wipeFiles: async () => {},
      });
      return sqlite3;
    },
  };
});

const API = `http://localhost:8787/${CURRENT_API_VERSION}`;
const BLOBS = 'https://blobs.test';
const VIEWER = TEST_USER.id;
const ZERO_STATE = '0'.repeat(64);

type VectorCase = {
  name: string;
  blob_hex: string;
  access_key_base64: string;
  end_hash_hex: string;
  recipient_private_key_hex: string;
  events: { ts: number; type: string; risk?: number }[];
};

const vectors = (
  JSON.parse(
    readFileSync(resolve(process.cwd(), '../batch-format/vectors/batch.json'), 'utf8'),
  ) as { cases: VectorCase[] }
).cases;
const vector = (name: string) => vectors.find((c) => c.name === name)!;
const hexToBytes = (hex: string) =>
  new Uint8Array(hex.match(/../g)!.map((byte) => parseInt(byte, 16)));

type Msg = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
const posted: Msg[] = [];
let nextId = 0;
let onMessage: (e: { data: Msg }) => Promise<void>;

// What the fake server holds for one test.
type ServedBatch = {
  id: string;
  device_id?: string;
  key: VectorCase;
  endHash?: string;
  blob?: Uint8Array | 'network-error' | 'timeout' | number;
  access_key?: string;
};
let served: ServedBatch[] = [];
let blobRequests: Record<string, number> = {};

function batchRow(b: ServedBatch, index: number) {
  return {
    id: b.id,
    user_id: VIEWER,
    device_id: b.device_id ?? 'device-1',
    start_time: Date.now() - 3_600_000,
    end_time: Date.now() - 1_800_000,
    end_hash: b.endHash ?? b.key.end_hash_hex,
    version: 'v0.1',
    url: `${BLOBS}/${b.id}`,
    encrypted_key: b.access_key ?? b.key.access_key_base64,
    created_at: Date.now() - 1000 * (index + 1),
  };
}

beforeAll(async () => {
  vi.stubGlobal('postMessage', (msg: Msg) => posted.push(msg));
  (self as unknown as { postMessage: unknown }).postMessage = (msg: Msg) => posted.push(msg);
  await import('./worker');
  onMessage = (self as unknown as { onmessage: typeof onMessage }).onmessage;
  // Any one-shot request waits for the database to open; clearCache would not.
  await reply({ method: 'getDecryptionStats', viewerId: VIEWER, targetUserId: VIEWER });
});

const keys = new Map<string, CryptoKey>();
/** The private key a vector's batch is sealed to, as the session would hold it. */
async function keyFor(c: VectorCase): Promise<CryptoKey> {
  if (!keys.has(c.recipient_private_key_hex)) {
    keys.set(
      c.recipient_private_key_hex,
      await importUserPrivateKey(hexToBytes(c.recipient_private_key_hex)),
    );
  }
  return keys.get(c.recipient_private_key_hex)!;
}

beforeEach(async () => {
  await reply({ method: 'clearCache' });
  posted.length = 0;
  served = [];
  blobRequests = {};
  server.use(
    http.get(`${API}/data`, () =>
      HttpResponse.json({
        batches: served.map(batchRow),
        user: TEST_USER,
        watching: [],
        watchers: [],
      }),
    ),
    http.get(`${BLOBS}/:id`, ({ params }) => {
      const id = params.id as string;
      blobRequests[id] = (blobRequests[id] ?? 0) + 1;
      const batch = served.find((b) => b.id === id)!;
      const blob = batch.blob ?? hexToBytes(batch.key.blob_hex);
      if (blob === 'network-error') return HttpResponse.error();
      if (typeof blob === 'number') return new HttpResponse(null, { status: blob });
      return new HttpResponse(blob as BodyInit);
    }),
  );
});

afterEach(() => {
  vi.restoreAllMocks();
});

async function send(req: Msg): Promise<void> {
  await onMessage({ data: { id: `req-${nextId++}`, ...req } });
}

async function reply(req: Msg): Promise<any> {
  // eslint-disable-line @typescript-eslint/no-explicit-any
  const id = `req-${nextId++}`;
  await onMessage({ data: { id, ...req } });
  const response = posted.find((m) => m.id === id && !m.type)!;
  if ('error' in response) throw new Error(response.error);
  return response.result;
}

/** Starts a session, queries the viewer's own logs, and waits for the sync to end. */
async function syncAndRead(options: { force?: boolean; deviceId?: string } = {}) {
  const id = `query-${nextId++}`;
  const done = () => posted.find((m) => m.type === 'queryChunk' && m.id === id && m.done);
  if (options.force) {
    await send({ method: 'refetch' });
  }
  await onMessage({
    data: {
      id,
      method: 'cacheQuery',
      query: { userId: VIEWER, deviceId: options.deviceId },
      targetUserId: VIEWER,
    },
  });
  await vi.waitFor(() => expect(done()).toBeDefined(), { timeout: 10_000 });
  return done() as { logs: FeedLog[]; error?: { kind: string; message: string } };
}

async function startSession(key: CryptoKey | null) {
  // setSession kicks off a forced sync, so it also resets the 1 minute freshness window.
  await send({ method: 'setSession', userId: VIEWER, privateKey: key });
}

async function stats(extra: Msg = {}) {
  return reply({ method: 'getDecryptionStats', viewerId: VIEWER, targetUserId: VIEWER, ...extra });
}

async function settle() {
  // Waits for the forced sync that setSession / refetch start to finish.
  await new Promise((r) => setTimeout(r, 200));
  await vi.waitFor(
    async () => {
      const s = await stats();
      expect(s.decryptedBatches + s.failedBatches).toBe(s.totalBatches);
    },
    { timeout: 10_000 },
  );
}

async function resetCache() {
  await reply({ method: 'clearCache' });
  posted.length = 0;
}

// Two vectors sealed to different recipient keys.
const KEY_A = () => vector('every_known_event_type'); // 01020304...
const KEY_B = () => vector('unknown_type_and_extra_keys'); // a0a7aeb5...

describe('batch decryption (WEB-016)', () => {
  it("decrypts every event of a batch sealed to the user's key (WEB-016)", async () => {
    const c = KEY_A();
    served = [{ id: 'b-1', key: c }];
    await startSession(await keyFor(c));
    const { logs, error } = await syncAndRead();
    expect(error).toBeUndefined();
    expect(logs.map((l) => l.type).sort()).toEqual(c.events.map((e) => e.type).sort());
    expect(logs.every((l) => l.device_id === 'device-1')).toBe(true);
    expect(logs.map((l) => l.ts).sort()).toEqual(c.events.map((e) => e.ts).sort());
  });

  it('shows events of a type the app does not know (WEB-016)', async () => {
    const c = KEY_B();
    served = [{ id: 'b-future', key: c }];
    await startSession(await keyFor(c));
    const { logs } = await syncAndRead();
    expect(logs.map((l) => l.type)).toEqual(['future_event']);
  });

  it('decrypts the batches of several devices together (WEB-016)', async () => {
    const a = vector('single_event_no_risk_no_data');
    const b = vector('unicode_strings');
    served = [
      { id: 'b-1', device_id: 'device-1', key: a },
      { id: 'b-2', device_id: 'device-2', key: b },
    ];
    await startSession(await keyFor(a));
    const { logs } = await syncAndRead();
    expect(logs.filter((l) => l.device_id === 'device-1')).toHaveLength(a.events.length);
    expect(logs.filter((l) => l.device_id === 'device-2')).toHaveLength(b.events.length);
  });

  it('decrypts nothing, and downloads no blob, while the private key is unavailable (WEB-016)', async () => {
    served = [{ id: 'b-1', key: KEY_A() }];
    await startSession(null);
    const { logs } = await syncAndRead();
    expect(logs).toEqual([]);
    expect(blobRequests['b-1']).toBeUndefined();
  });
});

describe('verified and unverified events (WEB-017)', () => {
  it('marks events from a batch whose hash chain matches as verified (WEB-017)', async () => {
    const c = KEY_A();
    served = [{ id: 'b-1', key: c }];
    await startSession(await keyFor(c));
    const { logs } = await syncAndRead();
    expect(logs).toHaveLength(c.events.length);
    expect(logs.every((l) => l.batch_status === 'verified')).toBe(true);
  });

  it('still returns events from a batch with a mismatched end hash, marked failed (WEB-017)', async () => {
    const c = KEY_A();
    served = [{ id: 'b-1', key: c, endHash: '1'.repeat(64) }];
    await startSession(await keyFor(c));
    const { logs } = await syncAndRead();
    expect(logs).toHaveLength(c.events.length);
    expect(logs.every((l) => l.batch_status === 'failed')).toBe(true);
  });

  it('marks only the tampered batch, not its neighbours (WEB-017)', async () => {
    const good = vector('single_event_no_risk_no_data');
    const bad = vector('unicode_strings');
    served = [
      { id: 'b-good', device_id: 'device-1', key: good },
      { id: 'b-bad', device_id: 'device-2', key: bad, endHash: ZERO_STATE },
    ];
    await startSession(await keyFor(good));
    const { logs } = await syncAndRead();
    expect(logs.filter((l) => l.device_id === 'device-1').map((l) => l.batch_status)).toEqual([
      'verified',
    ]);
    expect(logs.filter((l) => l.device_id === 'device-2')).toHaveLength(bad.events.length);
    expect(
      logs.filter((l) => l.device_id === 'device-2').every((l) => l.batch_status === 'failed'),
    ).toBe(true);
  });

  it('keeps the unverified mark after the events are read back from the cache (WEB-017)', async () => {
    const c = KEY_A();
    served = [{ id: 'b-1', key: c, endHash: '2'.repeat(64) }];
    await startSession(await keyFor(c));
    await syncAndRead();
    const again = await syncAndRead();
    expect(again.logs.every((l) => l.batch_status === 'failed')).toBe(true);
  });
});

describe('batches that cannot be decrypted (WEB-018)', () => {
  it('records a batch sealed to another key as failed, with its error, and shows none of its events (WEB-018)', async () => {
    const mine = KEY_A();
    const theirs = KEY_B();
    served = [{ id: 'b-theirs', key: theirs, access_key: theirs.access_key_base64 }];
    await startSession(await keyFor(mine));
    const { logs } = await syncAndRead();
    expect(logs).toEqual([]);
    const s = await stats();
    expect(s).toMatchObject({ totalBatches: 1, decryptedBatches: 0, failedBatches: 1 });
    expect(s.failureReasons).toHaveLength(1);
    expect(s.failureReasons[0].error).not.toBe('');
    expect(s.failureReasons[0].count).toBe(1);
  });

  it.each([
    ['a blob that is too short', () => new Uint8Array(5), /too short/],
    [
      'a blob with a flipped byte',
      () => {
        const blob = hexToBytes(KEY_A().blob_hex);
        blob[blob.length - 1] ^= 0xff;
        return blob;
      },
      /.+/,
    ],
    ['a blob that is not a batch at all', () => new Uint8Array(64).fill(7), /.+/],
  ])(
    'records %s as failed and contributes no events (WEB-018)',
    async (_name, makeBlob, message) => {
      const c = KEY_A();
      served = [{ id: 'b-bad', key: c, blob: makeBlob() }];
      await startSession(await keyFor(c));
      const { logs } = await syncAndRead();
      expect(logs).toEqual([]);
      const s = await stats();
      expect(s).toMatchObject({ decryptedBatches: 0, failedBatches: 1, totalEvents: 0 });
      expect(s.failureReasons[0].error).toMatch(message);
    },
  );

  it('keeps the events of readable batches when another batch fails (WEB-018)', async () => {
    const c = vector('single_event_no_risk_no_data');
    served = [
      { id: 'b-good', device_id: 'device-1', key: c },
      { id: 'b-bad', device_id: 'device-2', key: c, blob: new Uint8Array(3) },
    ];
    await startSession(await keyFor(c));
    const { logs } = await syncAndRead();
    expect(logs.map((l) => l.device_id)).toEqual(['device-1']);
    expect(await stats()).toMatchObject({ decryptedBatches: 1, failedBatches: 1, totalBatches: 2 });
  });

  it('does not try a failed batch again until the cache is cleared (WEB-018)', async () => {
    const c = KEY_A();
    served = [{ id: 'b-bad', key: c, blob: new Uint8Array(3) }];
    const key = await keyFor(c);
    await startSession(key);
    await syncAndRead();
    expect(blobRequests['b-bad']).toBe(1);

    await syncAndRead({ force: true });
    await send({ method: 'refetch' });
    await syncAndRead({ force: true });
    expect(blobRequests['b-bad']).toBe(1);

    // Clearing the cache forgets the failure, so the (now fixed) batch is downloaded again.
    await reply({ method: 'clearCache' });
    posted.length = 0;
    served = [{ id: 'b-bad', key: c }];
    await startSession(key);
    const { logs } = await syncAndRead();
    expect(blobRequests['b-bad']).toBe(2);
    expect(logs).toHaveLength(c.events.length);
  });

  it.each([
    ['a network failure', { blob: 'network-error' as const }],
    ['an HTTP error', { blob: 503 }],
  ])(
    'tries a batch again on the next sync after %s, and does not count it as failed (WEB-018)',
    async (_name, patch) => {
      const c = KEY_A();
      served = [{ id: 'b-flaky', key: c, ...patch }];
      const key = await keyFor(c);
      await startSession(key);
      const first = await syncAndRead();
      expect(first.logs).toEqual([]);
      expect(await stats()).toMatchObject({ failedBatches: 0, decryptedBatches: 0 });

      served = [{ id: 'b-flaky', key: c }];
      const second = await syncAndRead({ force: true });
      expect(blobRequests['b-flaky']).toBe(2);
      expect(second.logs).toHaveLength(c.events.length);
      expect(await stats()).toMatchObject({ failedBatches: 0, decryptedBatches: 1 });
    },
  );

  it('gives each batch download 20 seconds, and tries a timed-out batch again on the next sync (WEB-018)', async () => {
    const c = KEY_A();
    const realTimeout = AbortSignal.timeout.bind(AbortSignal);
    const timeouts: number[] = [];
    vi.spyOn(AbortSignal, 'timeout').mockImplementation((ms: number) => {
      timeouts.push(ms);
      return ms === 20_000
        ? AbortSignal.abort(new DOMException('The operation timed out.', 'TimeoutError'))
        : realTimeout(ms);
    });
    served = [{ id: 'b-slow', key: c }];
    await startSession(await keyFor(c));
    const first = await syncAndRead();
    expect(timeouts).toContain(20_000);
    expect(first.logs).toEqual([]);
    expect(await stats()).toMatchObject({ failedBatches: 0, decryptedBatches: 0 });

    vi.restoreAllMocks();
    const second = await syncAndRead({ force: true });
    expect(second.logs).toHaveLength(c.events.length);
  });

  it('reports each distinct error with its count, for all time and for the current filter (WEB-018)', async () => {
    const mine = KEY_A();
    const theirs = KEY_B();
    served = [
      { id: 'b-1', device_id: 'device-1', key: theirs },
      { id: 'b-2', device_id: 'device-1', key: theirs },
      { id: 'b-3', device_id: 'device-2', key: mine, blob: new Uint8Array(3) },
      { id: 'b-4', device_id: 'device-2', key: mine },
    ];
    await startSession(await keyFor(mine));
    await syncAndRead();

    const all = await stats();
    expect(all).toMatchObject({ totalBatches: 4, decryptedBatches: 1, failedBatches: 3 });
    expect(all.totalEvents).toBe(mine.events.length);
    expect(all.failureReasons.map((r: { count: number }) => r.count)).toEqual([2, 1]);

    const device1 = await stats({ deviceId: 'device-1' });
    expect(device1).toMatchObject({ totalBatches: 2, decryptedBatches: 0, failedBatches: 2 });
    expect(device1.totalEvents).toBe(0);
    expect(device1.failureReasons).toEqual([{ error: all.failureReasons[0].error, count: 2 }]);
  });
});
