// Golden vectors shared with the Rust client (client/core/tests/vectors.rs) and the hash
// server (hash-server/tests/vectors.rs). The JSON lives in batch-format/vectors/; every
// case is checked here against the production functions in crypto.ts and
// batch-materializer.ts. See BATCH-008 in batch-format/SPEC.md.
//
// Regenerate the Rust-made cases with the ignored `generate_vectors` test in
// client/core/tests/vectors.rs. The TypeScript-made cases (`producer` is `ts` or
// `ts-seed`) are regenerated with:
//   REGENERATE_TS_VECTORS=1 bun run test -- src/utils/api/vectors.test.ts
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { decode } from '@msgpack/msgpack';
import {
  decompressGzip,
  decryptBatch,
  decryptForOwnKey,
  derivePasswordMaterial,
  encryptForPublicKey,
  importUserPrivateKey,
  privateKeyMatchesPublicKey,
  unwrapBatchKey,
  verifyBatch,
} from './crypto';
import { decryptAndFlattenBatch } from './batch-materializer';
import { buildBatch, type SeedEvent } from '../../../../scripts/seed/batch';
import type { Batch, HashParams } from './api';

// vitest runs with web/ as the working directory.
const VECTORS_DIR = resolve(process.cwd(), '../batch-format/vectors');
const ZERO_STATE = '0'.repeat(64);

type Json = any; // eslint-disable-line @typescript-eslint/no-explicit-any

function loadVectors(name: string): { description: string; cases: Json[] } {
  return JSON.parse(readFileSync(resolve(VECTORS_DIR, name), 'utf8'));
}

function hexToBytes(hex: string): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(new ArrayBuffer(hex.length / 2));
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

function base64ToBytes(b64: string): Uint8Array<ArrayBuffer> {
  return new Uint8Array(Buffer.from(b64, 'base64'));
}

/** Replaces `{"$hex": "..."}` with bytes. */
function reviveBytes(value: Json): Json {
  if (Array.isArray(value)) return value.map(reviveBytes);
  if (value && typeof value === 'object') {
    const keys = Object.keys(value);
    if (keys.length === 1 && keys[0] === '$hex') return hexToBytes(value.$hex);
    return Object.fromEntries(keys.map((k) => [k, reviveBytes(value[k])]));
  }
  return value;
}

/** Floats are stored as msgpack float32, so a consumer sees them widened from float32. */
function widenFloats(value: Json): Json {
  const out = { ...value };
  for (const key of ['skin_detection', 'nsfw_detection']) {
    if (typeof out[key] === 'number') out[key] = Math.fround(out[key]);
  }
  return out;
}

function expectedLog(event: Json) {
  const revived = reviveBytes(event);
  const data = revived.data === undefined ? {} : widenFloats(revived.data);
  return {
    ts: revived.ts,
    type: revived.type,
    risk: typeof revived.risk === 'number' ? Math.fround(revived.risk) : undefined,
    data,
  };
}

async function importPrivateKey(hex: string) {
  return importUserPrivateKey(hexToBytes(hex));
}

async function openAs(
  privateKeyHex: string,
  envelopeBytes: Uint8Array<ArrayBuffer>,
): Promise<CryptoKey> {
  return unwrapBatchKey(await importPrivateKey(privateKeyHex), envelopeBytes);
}

async function consumeBatch(caseData: Json, endHash = caseData.end_hash_hex) {
  const blob = hexToBytes(caseData.blob_hex);
  const fetchSpy = vi
    .spyOn(globalThis, 'fetch')
    .mockImplementation(async () => new Response(blob as BodyInit));
  try {
    const batch = {
      id: 'batch-1',
      device_id: 'device-1',
      url: 'https://example.invalid/blob',
      encrypted_key: caseData.access_key_base64,
      end_hash: endHash,
      end_time: 1,
      created_at: 2,
    } as unknown as Batch;
    return await decryptAndFlattenBatch(
      batch,
      (encryptedKey) => openAs(caseData.recipient_private_key_hex, base64ToBytes(encryptedKey)),
      ZERO_STATE,
    );
  } finally {
    fetchSpy.mockRestore();
  }
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('event content hash (BATCH-006)', () => {
  // verifyBatch is the only TypeScript entry point for the content hash: one event from a
  // zero state ends at sha256(zeros || sha256(event)).
  const { cases } = loadVectors('event-hash.json');

  it.each(cases.map((c) => [c.name, c]))('%s', async (_name, c) => {
    const bytes = hexToBytes(c.encoded_hex);
    expect(createHash('sha256').update(bytes).digest('hex')).toBe(c.content_hash_hex);
    expect(await verifyBatch([bytes], ZERO_STATE, c.state_from_zero_hex)).toBe('verified');
    // A different event must not verify against this state.
    const other = new Uint8Array([...bytes, 0]);
    expect(await verifyBatch([other], ZERO_STATE, c.state_from_zero_hex)).toBe('failed');
  });
});

describe('rolling hash state (BATCH-006)', () => {
  const { cases } = loadVectors('rolling-hash.json');
  // Cases that only list content hashes cannot be fed to verifyBatch, which hashes events.
  const withEvents = cases.filter((c) => c.events_hex);

  it.each(withEvents.map((c) => [c.name, c]))('%s', async (_name, c) => {
    const events = c.events_hex.map(hexToBytes);
    expect(await verifyBatch(events, c.start_state_hex, c.end_state_hex)).toBe('verified');
    // Every prefix reaches the intermediate state.
    for (let i = 0; i < events.length; i++) {
      expect(await verifyBatch(events.slice(0, i + 1), c.start_state_hex, c.states_hex[i])).toBe(
        'verified',
      );
    }
    // Dropping, reordering or adding an event changes the end state.
    if (events.length > 0) {
      expect(await verifyBatch(events.slice(1), c.start_state_hex, c.end_state_hex)).toBe('failed');
      expect(await verifyBatch([...events, events[0]], c.start_state_hex, c.end_state_hex)).toBe(
        'failed',
      );
    }
  });
});

describe('batch blob (BATCH-002, BATCH-004, BATCH-005, BATCH-006)', () => {
  const { cases } = loadVectors('batch.json');

  it.each(cases.map((c) => [`${c.name} (${c.producer})`, c]))('consumes %s', async (_name, c) => {
    // BATCH-005: the access key opens to the batch key.
    const key = await openAs(c.recipient_private_key_hex, base64ToBytes(c.access_key_base64));
    // BATCH-002: AES-GCM, gzip, then a msgpack array of byte strings.
    const blob = hexToBytes(c.blob_hex);
    const plaintext = await decompressGzip(await decryptBatch(key, blob));
    const items = decode(plaintext) as unknown[];
    expect(items.length).toBe(c.encoded_events_hex.length);
    // BATCH-004: byte strings are bin, except in the legacy integer-array form.
    const wantBin = c.encoding !== 'legacy_int_arrays';
    for (const item of items) expect(item instanceof Uint8Array).toBe(wantBin);
    const eventBytes = items.map((item) =>
      item instanceof Uint8Array ? item : new Uint8Array(item as number[]),
    );
    expect(eventBytes.map(bytesToHex)).toEqual(c.encoded_events_hex);
    // BATCH-006: hash chain from zero.
    expect(await verifyBatch(eventBytes, ZERO_STATE, c.end_hash_hex)).toBe('verified');
    expect(eventBytes.map((e) => createHash('sha256').update(e).digest('hex'))).toEqual(
      c.content_hashes_hex,
    );
  });

  it.each(cases.filter((c) => c.events.length > 0).map((c) => [c.name, c]))(
    'decrypts and flattens %s into the logical events (BATCH-003)',
    async (_name, c) => {
      const logs = await consumeBatch(c);
      expect(logs.length).toBe(c.events.length);
      logs.forEach((log, i) => {
        const want = expectedLog(c.events[i]);
        expect(log.ts).toBe(want.ts);
        expect(log.type).toBe(want.type);
        expect(log.risk).toBe(want.risk);
        expect(log.data).toEqual(want.data);
        expect(log.batch_status).toBe('verified');
      });
    },
    30_000,
  );

  it.each(cases.filter((c) => c.events.length > 0).map((c) => [c.name, c]))(
    'marks %s failed when the end hash differs (BATCH-006)',
    async (_name, c) => {
      const logs = await consumeBatch(c, '1'.repeat(64));
      expect(logs.length).toBe(c.events.length);
      expect(logs.every((l) => l.batch_status === 'failed')).toBe(true);
    },
  );

  it('decrypts the empty event list to no events (BATCH-002)', async () => {
    const empty = cases.find((c) => c.name === 'empty_event_list')!;
    expect(await consumeBatch(empty)).toEqual([]);
    expect(await verifyBatch([], ZERO_STATE, empty.end_hash_hex)).toBe('verified');
  });
});

describe('HPKE key wrapping (BATCH-005)', () => {
  const { cases } = loadVectors('hpke.json');

  it.each(cases.map((c) => [`${c.name} (${c.producer}, ${c.expect})`, c]))('%s', async (_n, c) => {
    const privateKey = await importPrivateKey(c.recipient_private_key_hex);
    const envelope = hexToBytes(c.envelope_hex);
    if (c.expect === 'fail') {
      await expect(decryptForOwnKey(privateKey, envelope)).rejects.toThrow();
      await expect(unwrapBatchKey(privateKey, envelope)).rejects.toThrow();
      return;
    }
    expect(bytesToHex(await decryptForOwnKey(privateKey, envelope))).toBe(c.plaintext_hex);
    expect(
      await privateKeyMatchesPublicKey(privateKey, hexToBytes(c.recipient_public_key_hex)),
    ).toBe(true);

    // The opened batch key is usable: it decrypts a blob sealed under the same bytes.
    if (c.plaintext_hex.length === 64) {
      const key = await unwrapBatchKey(privateKey, envelope);
      const raw = await crypto.subtle.importKey(
        'raw',
        hexToBytes(c.plaintext_hex),
        'AES-GCM',
        false,
        ['encrypt'],
      );
      const iv = new Uint8Array(12);
      const sealed = await crypto.subtle.encrypt(
        { name: 'AES-GCM', iv },
        raw,
        new Uint8Array([1, 2]),
      );
      const opened = await crypto.subtle.decrypt({ name: 'AES-GCM', iv }, key, sealed);
      expect(Array.from(new Uint8Array(opened))).toEqual([1, 2]);
    }
  });

  it('produces enc || ciphertext of the documented size (BATCH-005)', async () => {
    const c = cases.find((x) => x.expect === 'ok' && x.plaintext_hex.length === 64)!;
    const sealed = await encryptForPublicKey(
      hexToBytes(c.recipient_public_key_hex),
      hexToBytes(c.plaintext_hex),
    );
    expect(sealed.length).toBe(32 + 32 + 16);
    const privateKey = await importPrivateKey(c.recipient_private_key_hex);
    expect(bytesToHex(await decryptForOwnKey(privateKey, sealed))).toBe(c.plaintext_hex);
  });
});

describe('password auth derivation (API-015, API-016)', () => {
  const { cases } = loadVectors('password-auth.json');

  async function derive(c: Json) {
    const params: HashParams = {
      version: 'test',
      algorithm: 'argon2id',
      memory_cost_kib: c.params.memory_cost_kib,
      time_cost: c.params.time_cost,
      parallelism: c.params.parallelism,
      salt_length: 16,
      hkdf_hash: 'sha256',
    };
    const { passwordAuth } = await derivePasswordMaterial(
      c.password,
      hexToBytes(c.salt_hex),
      params,
    );
    expect(bytesToHex(passwordAuth)).toBe(c.password_auth_hex);
    expect(Buffer.from(passwordAuth).toString('base64')).toBe(c.password_auth_base64);
  }

  it.each(cases.map((c) => [c.name, c]))('%s', async (_name, c) => derive(c), 120_000);

  it('rejects an empty password (BATCH-008)', async () => {
    const params: HashParams = {
      version: 'test',
      algorithm: 'argon2id',
      memory_cost_kib: 64,
      time_cost: 1,
      parallelism: 1,
      salt_length: 16,
      hkdf_hash: 'sha256',
    };
    await expect(derivePasswordMaterial('', new Uint8Array(16), params)).rejects.toThrow();
  });
});

describe('seed script batches (BATCH-002, BATCH-006)', () => {
  // scripts/seed/batch.ts is a second TypeScript producer of the batch format. Its output
  // must verify with the production consumer.
  it('builds batches that the production consumer decrypts and verifies', async () => {
    const batchCase = loadVectors('batch.json').cases.find(
      (c) => c.name === 'every_known_event_type',
    )!;
    const events = reviveBytes(batchCase.events) as SeedEvent[];
    const privateKey = hexToBytes(batchCase.recipient_private_key_hex);
    const publicKey = base64ToBytes(batchCase.recipient_public_key_base64);
    const built = await buildBatch(events, [{ uuid: 'recipient', pubKey: publicKey }]);

    const keyBytes = await decryptForOwnKey(
      await importUserPrivateKey(privateKey),
      base64ToBytes(built.accessKeys.recipient),
    );
    const key = await crypto.subtle.importKey('raw', new Uint8Array(keyBytes), 'AES-GCM', false, [
      'decrypt',
    ]);
    const payload = decode(
      await decompressGzip(await decryptBatch(key, built.blob)),
    ) as Uint8Array[];
    expect(await verifyBatch(payload, ZERO_STATE, built.endHashHex)).toBe('verified');
    const decoded = payload.map((p) => decode(p) as Json);
    expect(decoded.map((d) => d.type)).toEqual(events.map((e) => e.type));
    expect(decoded.map((d) => d.ts)).toEqual(events.map((e) => e.ts));
  });
});

// Generator for the TypeScript-made cases, which the Rust tests consume. Skipped unless
// REGENERATE_TS_VECTORS is set.
describe.skipIf(!process.env.REGENERATE_TS_VECTORS)(
  'regenerate TypeScript vectors (BATCH-008)',
  () => {
    function writeMerged(file: string, produced: Json[], keep: (c: Json) => boolean) {
      const url = resolve(VECTORS_DIR, file);
      const existing = JSON.parse(readFileSync(url, 'utf8'));
      existing.cases = [...existing.cases.filter(keep), ...produced];
      writeFileSync(url, JSON.stringify(existing, null, 2) + '\n');
    }

    it('hpke.json: envelopes sealed by encryptForPublicKey', async () => {
      const hpke = loadVectors('hpke.json').cases;
      const rustCases = hpke.filter((c) => c.producer === 'rust' && c.expect === 'ok');
      const produced: Json[] = [];
      const plaintexts: [string, Uint8Array][] = [
        ['ts_sealed_batch_key', hexToBytes('0f'.repeat(32))],
        ['ts_sealed_sequential_batch_key', Uint8Array.from({ length: 32 }, (_, i) => 255 - i)],
        ['ts_sealed_locked_password_20_bytes', new TextEncoder().encode('locked pass é日本')],
      ];
      for (const [i, [name, plaintext]] of plaintexts.entries()) {
        const recipient = rustCases[i % 2];
        const sealed = await encryptForPublicKey(
          hexToBytes(recipient.recipient_public_key_hex),
          new Uint8Array(plaintext),
        );
        produced.push({
          name,
          producer: 'ts',
          expect: 'ok',
          recipient_private_key_hex: recipient.recipient_private_key_hex,
          recipient_public_key_hex: recipient.recipient_public_key_hex,
          plaintext_hex: bytesToHex(plaintext),
          envelope_hex: bytesToHex(sealed),
        });
      }
      writeMerged('hpke.json', produced, (c) => c.producer !== 'ts');
    });

    it('batch.json: batches built by scripts/seed/batch.ts', async () => {
      const rustCase = loadVectors('batch.json').cases.find(
        (c) => c.name === 'single_event_no_risk_no_data',
      )!;
      const privateKey = await importUserPrivateKey(hexToBytes(rustCase.recipient_private_key_hex));
      const publicKey = base64ToBytes(rustCase.recipient_public_key_base64);
      const sets: [string, Json[]][] = [
        [
          'seed_script_mixed_events',
          [
            { ts: 1700000000000, type: 'heartbeat' },
            { ts: 1700000001000, risk: 0.5, type: 'alert', data: { message: 'naïve 日本語' } },
            {
              ts: 1700000002000,
              risk: 0.25,
              type: 'screenshot',
              data: {
                image: { $hex: bytesToHex(Uint8Array.from({ length: 300 }, (_, i) => i % 256)) },
                content_type: 'image/webp',
              },
            },
          ],
        ],
      ];
      const produced: Json[] = [];
      for (const [name, events] of sets) {
        const built = await buildBatch(reviveBytes(events) as SeedEvent[], [
          { uuid: 'recipient', pubKey: publicKey },
        ]);
        const accessKey = base64ToBytes(built.accessKeys.recipient);
        const keyBytes = await decryptForOwnKey(privateKey, accessKey);
        const key = await crypto.subtle.importKey(
          'raw',
          new Uint8Array(keyBytes),
          'AES-GCM',
          false,
          ['decrypt'],
        );
        const payload = decode(
          await decompressGzip(await decryptBatch(key, built.blob)),
        ) as Uint8Array[];
        produced.push({
          name,
          note: 'Built by scripts/seed/batch.ts. msgpack-js encodes risk as float64 (or an integer when whole), unlike the Rust producer.',
          producer: 'ts-seed',
          encoding: 'seed_js_msgpack',
          events,
          encoded_events_hex: payload.map(bytesToHex),
          content_hashes_hex: payload.map((p) => createHash('sha256').update(p).digest('hex')),
          end_hash_hex: built.endHashHex,
          recipient_private_key_hex: rustCase.recipient_private_key_hex,
          recipient_public_key_base64: rustCase.recipient_public_key_base64,
          access_key_base64: Buffer.from(accessKey).toString('base64'),
          batch_key_hex: bytesToHex(keyBytes),
          blob_hex: bytesToHex(built.blob),
        });
      }
      writeMerged('batch.json', produced, (c) => c.producer === 'rust');
    });
  },
);
