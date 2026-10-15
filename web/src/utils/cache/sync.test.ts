import { describe, expect, it, vi } from 'vitest';
import {
  createSharedFetch,
  isSyncFresh,
  needsDeviceOwners,
  needsRerun,
  scopeBatchesToTarget,
  SHARE_SINCE_SPREAD_MS,
  SYNC_FRESH_MS,
} from './sync';

describe('isSyncFresh', () => {
  it('skips a sync within the fresh window unless forced', () => {
    expect(isSyncFresh(1_000, 1_000 + SYNC_FRESH_MS - 1, false)).toBe(true);
    expect(isSyncFresh(1_000, 1_000 + SYNC_FRESH_MS - 1, true)).toBe(false);
    expect(isSyncFresh(1_000, 1_000 + SYNC_FRESH_MS, false)).toBe(false);
    expect(isSyncFresh(undefined, 1_000, false)).toBe(false);
  });
});

describe('needsRerun', () => {
  const running = { forced: false, epoch: 1, hadKey: true };

  it('folds a plain sync into the running one', () => {
    expect(needsRerun(running, { force: false, epoch: 1, hasKey: true })).toBe(false);
  });

  it('reruns a forced sync when the running one may have skipped the network', () => {
    expect(needsRerun(running, { force: true, epoch: 1, hasKey: true })).toBe(true);
    expect(needsRerun({ ...running, forced: true }, { force: true, epoch: 1, hasKey: true })).toBe(
      false,
    );
  });

  it('reruns for a new session', () => {
    expect(needsRerun(running, { force: false, epoch: 2, hasKey: true })).toBe(true);
  });

  it('reruns when the private key arrived after the running fetch started', () => {
    expect(
      needsRerun({ ...running, hadKey: false }, { force: false, epoch: 1, hasKey: true }),
    ).toBe(true);
  });
});

describe('scopeBatchesToTarget', () => {
  const owners = new Map([
    ['dev-a', 'alice'],
    ['dev-b', 'bob'],
  ]);

  it('uses user_id when present and the device owner otherwise', () => {
    const batches = [
      { id: '1', user_id: 'alice', device_id: 'dev-b' },
      { id: '2', device_id: 'dev-a' },
      { id: '3', device_id: 'dev-b' },
      { id: '4', user_id: 'bob', device_id: 'dev-a' },
    ];
    expect(needsDeviceOwners(batches)).toBe(true);
    expect(scopeBatchesToTarget(batches, 'alice', owners).map((b) => b.id)).toEqual(['1', '2']);
    expect(scopeBatchesToTarget(batches, 'bob', owners).map((b) => b.id)).toEqual(['3', '4']);
  });

  it('needs no device lookup when every batch names its owner', () => {
    const batches = [{ id: '1', user_id: 'alice', device_id: 'dev-a' }];
    expect(needsDeviceOwners(batches)).toBe(false);
    expect(scopeBatchesToTarget(batches, 'alice', null)).toHaveLength(1);
  });

  it('drops batches whose owner cannot be found', () => {
    expect(scopeBatchesToTarget([{ id: '1', device_id: 'dev-x' }], 'alice', owners)).toEqual([]);
  });
});

describe('createSharedFetch', () => {
  it('shares one request among syncs started together, from the lowest since', async () => {
    const fetchSince = vi.fn(async (since: number) => since);
    const shared = createSharedFetch(fetchSince);
    const results = await Promise.all([shared(5_000), shared(3_000), shared(4_000)]);
    expect(fetchSince).toHaveBeenCalledTimes(1);
    expect(fetchSince).toHaveBeenCalledWith(3_000);
    expect(results).toEqual([3_000, 3_000, 3_000]);
  });

  it('gives a sync from scratch its own request', async () => {
    const now = 100 * SHARE_SINCE_SPREAD_MS;
    const fetchSince = vi.fn(async (since: number) => since);
    const shared = createSharedFetch(fetchSince);
    const [warm, cold] = await Promise.all([shared(now - 60_000), shared(0)]);
    expect(fetchSince).toHaveBeenCalledTimes(2);
    expect(warm).toBe(now - 60_000);
    expect(cold).toBe(0);
  });

  it('keeps a failed request from failing the other group', async () => {
    const now = 100 * SHARE_SINCE_SPREAD_MS;
    const shared = createSharedFetch(async (since: number) => {
      if (since === 0) throw new Error('offline');
      return since;
    });
    const [warm, cold] = await Promise.allSettled([shared(now), shared(0)]);
    expect(warm).toEqual({ status: 'fulfilled', value: now });
    expect(cold.status).toBe('rejected');
  });

  it('starts a new request once the shared one has gone out', async () => {
    const fetchSince = vi.fn(async (since: number) => since);
    const shared = createSharedFetch(fetchSince);
    await shared(1_000);
    await shared(1_000);
    expect(fetchSince).toHaveBeenCalledTimes(2);
  });
});
