// Pure pieces of the cache worker's /data sync, kept out of worker.ts so they can be tested
// without SQLite or a worker.

// Batches only arrive about hourly, so a sync within this long of the last one skips the
// request unless it is forced.
export const SYNC_FRESH_MS = 60_000;

export function isSyncFresh(syncedAt: number | undefined, now: number, force: boolean): boolean {
  return !force && syncedAt !== undefined && now - syncedAt < SYNC_FRESH_MS;
}

// Syncs of batch rows alone (no download or decrypt), one per viewer and target at a time.
// A sync only runs when it hits the network, so one already running for this session
// serves a forced request too; one from an earlier session (see `epoch`) doesn't, and its
// late finish doesn't mark the target fresh. A target synced within SYNC_FRESH_MS is left
// alone unless forced.
export function createRowSync(options: {
  fetchRows: (viewerId: string, targetUserId: string) => Promise<void>;
  epoch: () => number;
  now?: () => number;
}) {
  const now = options.now ?? Date.now;
  const inFlight = new Map<string, { promise: Promise<void>; epoch: number }>();
  const syncedAt = new Map<string, number>();
  const key = (viewerId: string, targetUserId: string) => `${viewerId}:${targetUserId}`;

  return {
    sync(viewerId: string, targetUserId: string, force: boolean): Promise<void> {
      const k = key(viewerId, targetUserId);
      const running = inFlight.get(k);
      if (running && running.epoch === options.epoch()) return running.promise;
      if (isSyncFresh(syncedAt.get(k), now(), force)) return Promise.resolve();
      const entry = { epoch: options.epoch(), promise: Promise.resolve() };
      entry.promise = options
        .fetchRows(viewerId, targetUserId)
        .then(() => {
          if (entry.epoch === options.epoch()) syncedAt.set(k, now());
        })
        .finally(() => {
          if (inFlight.get(k) === entry) inFlight.delete(k);
        });
      inFlight.set(k, entry);
      return entry.promise;
    },
    // A new session syncs every target again.
    forgetSynced() {
      syncedAt.clear();
    },
    reset() {
      inFlight.clear();
      syncedAt.clear();
    },
  };
}

// What a running fetch for a target was started with.
export type RunningFetch = { forced: boolean; epoch: number; hadKey: boolean };

// A sync asked for while another runs for the same target is folded into it, unless the
// running one can't stand in for it: it may skip the network as fresh when this one is
// forced, belong to an earlier session, or have started without the private key it now
// needs to decrypt. Then the target syncs once more after it.
export function needsRerun(
  running: RunningFetch,
  request: { force: boolean; epoch: number; hasKey: boolean },
): boolean {
  return (
    (request.force && !running.forced) ||
    running.epoch !== request.epoch ||
    (!running.hadKey && request.hasKey)
  );
}

// GET /data bundles every target's batches. Each names its owner in `user_id`; an API that
// predates that field leaves it out, and the owner then comes from the batch's device.
export function needsDeviceOwners(batches: { user_id?: string }[]): boolean {
  return !batches.every((batch) => batch.user_id);
}

export function scopeBatchesToTarget<T extends { user_id?: string; device_id: string }>(
  batches: T[],
  targetUserId: string,
  deviceOwners: Map<string, string> | null,
): T[] {
  return batches.filter(
    (batch) => (batch.user_id ?? deviceOwners?.get(batch.device_id)) === targetUserId,
  );
}

// Syncs started in the same tick share one /data request, made with the lowest `since` any
// of them needs. Sharing is limited to requests whose `since` values lie within
// SHARE_SINCE_SPREAD_MS of each other, so a target syncing from scratch (`since` 0, such as
// a newly accepted partner) gets its own request instead of making every other target
// download 30 days of batches, and a failure of one request doesn't fail the others.
export const SHARE_SINCE_SPREAD_MS = 24 * 60 * 60 * 1000;

export function createSharedFetch<T>(
  fetchSince: (since: number) => Promise<T>,
  maxSpread = SHARE_SINCE_SPREAD_MS,
): (since: number) => Promise<T> {
  let pending: { min: number; max: number; promise: Promise<T> }[] = [];

  return (since: number) => {
    const group = pending.find((g) => Math.max(g.max, since) - Math.min(g.min, since) <= maxSpread);
    if (group) {
      group.min = Math.min(group.min, since);
      group.max = Math.max(group.max, since);
      return group.promise;
    }
    const created = { min: since, max: since, promise: null as unknown as Promise<T> };
    created.promise = Promise.resolve().then(() => {
      pending = pending.filter((g) => g !== created);
      return fetchSince(created.min);
    });
    pending.push(created);
    return created.promise;
  };
}
