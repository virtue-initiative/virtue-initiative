// Property test for the cache sync throttle (WEB-013).
import { describe, expect, it } from 'vitest';
import fc from 'fast-check';
import { createRowSync, SYNC_FRESH_MS } from './sync';

// One request: which person is synced, how long after the previous request, and whether
// the user asked for fresh data.
const requestArb = fc.record({
  target: fc.constantFrom('a', 'b', 'c'),
  waitMs: fc.integer({ min: 0, max: 2 * SYNC_FRESH_MS }),
  force: fc.boolean(),
});

describe('createRowSync (WEB-013)', () => {
  it('asks the server again for a person only when forced or not synced within the last minute', async () => {
    await fc.assert(
      fc.asyncProperty(fc.array(requestArb, { maxLength: 40 }), async (requests) => {
        let clock = 1_000_000;
        const fetched: string[] = [];
        const sync = createRowSync({
          fetchRows: async (_viewer, target) => {
            fetched.push(target);
          },
          epoch: () => 0,
          now: () => clock,
        });

        const lastSynced = new Map<string, number>();
        const expected: string[] = [];
        for (const { target, waitMs, force } of requests) {
          clock += waitMs;
          const last = lastSynced.get(target);
          if (force || last === undefined || clock - last >= SYNC_FRESH_MS) {
            expected.push(target);
            lastSynced.set(target, clock);
          }
          await sync.sync('viewer', target, force);
        }
        expect(fetched).toEqual(expected);
      }),
      { numRuns: 100 },
    );
  });
});
