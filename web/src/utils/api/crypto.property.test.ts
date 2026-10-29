// Property tests for the hash chain a consumer checks (BATCH-006). The golden vectors are in
// vectors.test.ts.
import { createHash } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import fc from 'fast-check';
import { verifyBatch } from './crypto';

const ZERO_STATE = '0'.repeat(64);

function sha256(...parts: Uint8Array[]): Uint8Array {
  const hash = createHash('sha256');
  for (const part of parts) hash.update(part);
  return new Uint8Array(hash.digest());
}

// The rule of BATCH-006, written out independently of crypto.ts.
function chainEnd(events: Uint8Array[]): string {
  let state: Uint8Array = new Uint8Array(32);
  for (const event of events) state = sha256(state, sha256(event));
  return Buffer.from(state).toString('hex');
}

const eventArb = fc.uint8Array({ maxLength: 64 });
const eventsArb = fc.array(eventArb, { minLength: 1, maxLength: 20 });
const runs = { numRuns: 100 };

describe('verifyBatch (BATCH-006)', () => {
  it('verifies the chain built with the rolling-hash rule', async () => {
    await fc.assert(
      fc.asyncProperty(eventsArb, async (events) => {
        expect(await verifyBatch(events, ZERO_STATE, chainEnd(events))).toBe('verified');
      }),
      runs,
    );
  });

  it('fails when any byte of any event is changed', async () => {
    await fc.assert(
      fc.asyncProperty(
        eventsArb.filter((events) => events.some((e) => e.length > 0)),
        fc.nat(),
        fc.nat(),
        fc.integer({ min: 1, max: 255 }),
        async (events, eventPick, bytePick, flip) => {
          const end = chainEnd(events);
          const candidates = events.flatMap((e, i) => (e.length > 0 ? [i] : []));
          const index = candidates[eventPick % candidates.length]!;
          const changed = events.map((e) => new Uint8Array(e));
          changed[index]![bytePick % changed[index]!.length]! ^= flip;
          expect(await verifyBatch(changed, ZERO_STATE, end)).toBe('failed');
        },
      ),
      runs,
    );
  });

  it('fails when an event is dropped', async () => {
    await fc.assert(
      fc.asyncProperty(eventsArb, fc.nat(), async (events, pick) => {
        const end = chainEnd(events);
        const kept = events.filter((_, i) => i !== pick % events.length);
        expect(await verifyBatch(kept, ZERO_STATE, end)).toBe('failed');
      }),
      runs,
    );
  });

  it('fails when an event is appended', async () => {
    await fc.assert(
      fc.asyncProperty(eventsArb, eventArb, async (events, extra) => {
        const end = chainEnd(events);
        expect(await verifyBatch([...events, extra], ZERO_STATE, end)).toBe('failed');
      }),
      runs,
    );
  });

  it('fails when the events are reordered', async () => {
    await fc.assert(
      fc.asyncProperty(eventsArb, fc.nat(), fc.nat(), async (events, a, b) => {
        const i = a % events.length;
        const j = b % events.length;
        const swapped = [...events];
        [swapped[i], swapped[j]] = [swapped[j]!, swapped[i]!];
        // Swapping two equal events gives the same sequence, which must still verify.
        const same = swapped.every((e, k) => Buffer.compare(e, events[k]!) === 0);
        const result = await verifyBatch(swapped, ZERO_STATE, chainEnd(events));
        expect(result).toBe(same ? 'verified' : 'failed');
      }),
      runs,
    );
  });

  it('accepts events as any view over the same bytes', async () => {
    await fc.assert(
      fc.asyncProperty(eventsArb, async (events) => {
        const views = events.map((e) => {
          const padded = new Uint8Array(e.length + 8);
          padded.set(e, 4);
          return padded.subarray(4, 4 + e.length);
        });
        expect(await verifyBatch(views, ZERO_STATE, chainEnd(events))).toBe('verified');
      }),
      runs,
    );
  });
});
