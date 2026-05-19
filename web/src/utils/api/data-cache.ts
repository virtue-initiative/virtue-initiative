import Dexie, { Table } from 'dexie';
import { Batch, DataLog, DataPage } from './api';
import { FeedLog } from '../../pages/Logs/shared';

export interface CachedDataFeed {
  key: string;
  viewer_id: string;
  target_user_id: string;
  since: number;
  batches: Batch[];
  logs: DataLog[];
  materialized_batch_ids: string[];
}

interface StoredDecryptedEvent extends FeedLog {
  viewer_id: string;
}

interface StoredEventImage {
  id: string;
  viewer_id: string;
  data: Uint8Array;
}

export interface DecryptedEventQuery {
  deviceId?: string;
  allowedDeviceIds?: string[];
  startTs?: number;
  endTs?: number;
}

class VirtueDB extends Dexie {
  feeds!: Table<CachedDataFeed, string>;
  decryptedEvents!: Table<StoredDecryptedEvent, string>;
  eventImages!: Table<StoredEventImage, string>;

  constructor() {
    super('virtue-data-cache');
    this.version(3)
      .stores({
        feeds: 'key',
        decryptedEvents: 'id, [viewer_id+ts], [viewer_id+device_id]',
      })
      .upgrade((tx) =>
        tx
          .table('feeds')
          .toCollection()
          .modify((feed) => {
            feed.materialized_batch_ids ??= [];
          }),
      );
    this.version(4)
      .stores({
        eventImages: 'id, viewer_id',
      })
      .upgrade(async (tx) => {
        const events = await tx.table('decryptedEvents').toArray();
        for (const event of events) {
          const img = event.data?.image;
          if (!img) continue;
          const imgBytes =
            img instanceof Uint8Array ? img : Array.isArray(img) ? new Uint8Array(img) : undefined;
          if (!imgBytes) continue;
          await tx
            .table('eventImages')
            .put({ id: event.id, viewer_id: event.viewer_id, data: imgBytes });
          const newData = { ...event.data };
          delete newData.image;
          await tx.table('decryptedEvents').put({ ...event, data: newData });
        }
      });
  }
}

const db = new VirtueDB();

function feedKey(viewerId: string, targetUserId: string) {
  return `${viewerId}:${targetUserId}`;
}

function emptyFeed(viewerId: string, targetUserId: string): CachedDataFeed {
  return {
    key: feedKey(viewerId, targetUserId),
    viewer_id: viewerId,
    target_user_id: targetUserId,
    since: 0,
    batches: [],
    logs: [],
    materialized_batch_ids: [],
  };
}

function sortByCreatedAt<T extends { created_at: number }>(items: T[]) {
  items.sort((a, b) => a.created_at - b.created_at);
}

async function getFeed(viewerId: string, targetUserId: string): Promise<CachedDataFeed> {
  const feed = await db.feeds.get(feedKey(viewerId, targetUserId));
  if (!feed) return emptyFeed(viewerId, targetUserId);
  feed.materialized_batch_ids ??= [];
  return feed;
}

export async function loadCachedDataFeed(
  viewerId: string,
  targetUserId: string,
): Promise<CachedDataFeed> {
  return getFeed(viewerId, targetUserId);
}

export async function mergeDataPageIntoCache(
  viewerId: string,
  targetUserId: string,
  page: DataPage,
): Promise<CachedDataFeed> {
  const existing = await getFeed(viewerId, targetUserId);

  const batchIds = new Set(existing.batches.map((b) => b.id));
  for (const batch of page.batches) {
    if (!batchIds.has(batch.id)) {
      existing.batches.push(batch);
      batchIds.add(batch.id);
    }
  }

  const logIds = new Set(existing.logs.map((l) => l.id));
  for (const log of page.logs) {
    if (!logIds.has(log.id)) {
      existing.logs.push(log);
      logIds.add(log.id);
    }
  }

  sortByCreatedAt(existing.batches);
  sortByCreatedAt(existing.logs);

  const latestCreatedAt = Math.max(
    existing.since,
    ...page.batches.map((b) => b.created_at),
    ...page.logs.map((l) => l.created_at),
  );
  existing.since = Number.isFinite(latestCreatedAt) ? latestCreatedAt : existing.since;

  await db.feeds.put(existing);
  return existing;
}

export async function pruneCachedDataFeedDevices(
  viewerId: string,
  targetUserId: string,
  deviceIds: string[],
): Promise<CachedDataFeed> {
  const existing = await getFeed(viewerId, targetUserId);
  const allowed = new Set(deviceIds);
  const next: CachedDataFeed = {
    ...existing,
    batches: existing.batches.filter((b) => allowed.has(b.device_id)),
    logs: existing.logs.filter((l) => allowed.has(l.device_id)),
  };
  if (
    next.batches.length !== existing.batches.length ||
    next.logs.length !== existing.logs.length
  ) {
    await db.feeds.put(next);
    return next;
  }
  return existing;
}

export async function removeDeviceFromCachedDataFeed(
  viewerId: string,
  targetUserId: string,
  deviceId: string,
): Promise<CachedDataFeed> {
  const existing = await getFeed(viewerId, targetUserId);
  const next: CachedDataFeed = {
    ...existing,
    batches: existing.batches.filter((b) => b.device_id !== deviceId),
    logs: existing.logs.filter((l) => l.device_id !== deviceId),
  };
  if (
    next.batches.length !== existing.batches.length ||
    next.logs.length !== existing.logs.length
  ) {
    await db.feeds.put(next);
    return next;
  }
  return existing;
}

export async function clearDataCache(): Promise<void> {
  if (typeof indexedDB === 'undefined') return;
  await db.feeds.clear();
  await db.decryptedEvents.clear();
  await db.eventImages.clear();
}

// Returns batches that haven't been decrypted yet and are within the cutoff window.
// Pure function — no IDB access needed since materialized IDs live in the feed.
export function getUnmaterializedBatches(
  feed: CachedDataFeed,
  batches: Batch[],
  cutoffTs: number,
): Batch[] {
  const done = new Set(feed.materialized_batch_ids);
  return batches.filter((b) => !done.has(b.id) && b.created_at >= cutoffTs);
}

export async function writeMaterializedEvents(
  viewerId: string,
  targetUserId: string,
  batchId: string,
  events: FeedLog[],
): Promise<void> {
  await db.transaction('rw', [db.decryptedEvents, db.eventImages, db.feeds], async () => {
    for (const event of events) {
      const imageData = event.data.image instanceof Uint8Array ? event.data.image : undefined;
      const dataWithoutImage = { ...event.data };
      delete dataWithoutImage.image;
      await db.decryptedEvents.put({ ...event, data: dataWithoutImage, viewer_id: viewerId });
      if (imageData) {
        await db.eventImages.put({ id: event.id, viewer_id: viewerId, data: imageData });
      }
    }
    const feed = await getFeed(viewerId, targetUserId);
    if (!feed.materialized_batch_ids.includes(batchId)) {
      await db.feeds.put({
        ...feed,
        materialized_batch_ids: [...feed.materialized_batch_ids, batchId],
      });
    }
  });
}

export async function loadEventImage(
  viewerId: string,
  eventId: string,
): Promise<Uint8Array | undefined> {
  const record = await db.eventImages.get(eventId);
  if (!record || record.viewer_id !== viewerId) return undefined;
  return record.data;
}

export async function queryDecryptedEvents(
  viewerId: string,
  { deviceId, allowedDeviceIds, startTs, endTs }: DecryptedEventQuery,
): Promise<FeedLog[]> {
  let records: StoredDecryptedEvent[];

  if (startTs !== undefined && endTs !== undefined) {
    records = await db.decryptedEvents
      .where('[viewer_id+ts]')
      .between([viewerId, startTs], [viewerId, endTs], true, true)
      .toArray();
  } else {
    records = await db.decryptedEvents
      .where('[viewer_id+ts]')
      .between([viewerId, 0], [viewerId, Number.MAX_SAFE_INTEGER], true, true)
      .toArray();
  }

  let filtered: StoredDecryptedEvent[];
  if (deviceId) {
    filtered = records.filter((r) => r.device_id === deviceId);
  } else if (allowedDeviceIds) {
    const allowed = new Set(allowedDeviceIds);
    filtered = records.filter((r) => allowed.has(r.device_id));
  } else {
    filtered = records;
  }

  return filtered.map(({ viewer_id: _v, ...event }) => event as FeedLog);
}

export async function deleteDecryptedEventsForDevice(
  viewerId: string,
  deviceId: string,
): Promise<void> {
  const toDelete = (await db.decryptedEvents
    .where('[viewer_id+device_id]')
    .equals([viewerId, deviceId])
    .primaryKeys()) as string[];
  await db.decryptedEvents.bulkDelete(toDelete);
  await db.eventImages.bulkDelete(toDelete);
  const feed = await getFeed(viewerId, viewerId);
  if (feed) {
    const deviceBatchIds = new Set(
      feed.batches.filter((b) => b.device_id === deviceId).map((b) => b.id),
    );
    if (deviceBatchIds.size > 0) {
      await db.feeds.put({
        ...feed,
        materialized_batch_ids: feed.materialized_batch_ids.filter((id) => !deviceBatchIds.has(id)),
      });
    }
  }
}

export async function pruneDecryptedEventsBefore(
  viewerId: string,
  cutoffTs: number,
): Promise<void> {
  const toDelete = (await db.decryptedEvents
    .where('[viewer_id+ts]')
    .between([viewerId, 0], [viewerId, cutoffTs], true, false)
    .primaryKeys()) as string[];
  await db.decryptedEvents.bulkDelete(toDelete);
  await db.eventImages.bulkDelete(toDelete);
}
