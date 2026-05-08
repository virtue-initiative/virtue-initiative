import { Batch, DataLog, DataPage } from "./api";
import { FeedLog } from "./pages/Logs/shared";
import { decodeWebpDimensions } from "./utils/webp-dimensions";

const DB_NAME = "virtue-data-cache";
const DB_VERSION = 2;
const FEEDS_STORE = "feeds";
const DECRYPTED_EVENTS_STORE = "decrypted_events";
const MATERIALIZED_BATCHES_STORE = "materialized_batches";

export interface CachedDataFeed {
  key: string;
  viewer_id: string;
  target_user_id: string;
  since: number;
  batches: Batch[];
  logs: DataLog[];
}

interface StoredDecryptedEvent extends FeedLog {
  viewer_id: string;
}

interface MaterializedBatchRecord {
  id: string; // `${viewer_id}:${batch_id}`
  viewer_id: string;
  batch_id: string;
  device_id: string;
  created_at: number;
}

export interface DecryptedEventQuery {
  deviceId?: string;
  startTs?: number;
  endTs?: number;
}

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
  };
}

function requestToPromise<T>(request: IDBRequest<T>) {
  return new Promise<T>((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

function transactionDone(tx: IDBTransaction) {
  return new Promise<void>((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
}

function openDatabase() {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = (event) => {
      const db = request.result;
      const oldVersion = event.oldVersion;

      if (oldVersion < 1) {
        db.createObjectStore(FEEDS_STORE, { keyPath: "key" });
      }

      if (oldVersion < 2) {
        const eventsStore = db.createObjectStore(DECRYPTED_EVENTS_STORE, {
          keyPath: "id",
        });
        eventsStore.createIndex("by_viewer_ts", ["viewer_id", "ts"]);
        eventsStore.createIndex("by_viewer_device", ["viewer_id", "device_id"]);
        eventsStore.createIndex("by_device_id", "device_id");

        const batchesStore = db.createObjectStore(MATERIALIZED_BATCHES_STORE, {
          keyPath: "id",
        });
        batchesStore.createIndex("by_viewer", "viewer_id");
        batchesStore.createIndex("by_viewer_device", [
          "viewer_id",
          "device_id",
        ]);
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function withDatabase<T>(fn: (db: IDBDatabase) => Promise<T>) {
  if (typeof indexedDB === "undefined") {
    throw new Error("IndexedDB is not available in this browser");
  }

  const db = await openDatabase();
  try {
    return await fn(db);
  } finally {
    db.close();
  }
}

function sortByCreatedAt<T extends { created_at: number }>(items: T[]) {
  items.sort((a, b) => a.created_at - b.created_at);
}

function getAllFromIndex<T>(
  store: IDBObjectStore,
  indexName: string,
  range?: IDBKeyRange,
): Promise<T[]> {
  return requestToPromise(
    range
      ? store.index(indexName).getAll(range)
      : store.index(indexName).getAll(),
  ) as Promise<T[]>;
}

function deleteByIndexCursor(
  store: IDBObjectStore,
  indexName: string,
  range: IDBKeyRange,
): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    const request = store.index(indexName).openCursor(range);
    request.onsuccess = () => {
      const cursor = request.result;
      if (cursor) {
        cursor.delete();
        cursor.continue();
      } else {
        resolve();
      }
    };
    request.onerror = () => reject(request.error);
  });
}

// ── Existing feed functions ────────────────────────────────────────────────

export async function loadCachedDataFeed(
  viewerId: string,
  targetUserId: string,
): Promise<CachedDataFeed> {
  return withDatabase(async (db) => {
    const tx = db.transaction(FEEDS_STORE, "readonly");
    const store = tx.objectStore(FEEDS_STORE);
    const cached = await requestToPromise<CachedDataFeed | undefined>(
      store.get(feedKey(viewerId, targetUserId)),
    );
    await transactionDone(tx);
    return cached ?? emptyFeed(viewerId, targetUserId);
  });
}

export async function mergeDataPageIntoCache(
  viewerId: string,
  targetUserId: string,
  page: DataPage,
): Promise<CachedDataFeed> {
  return withDatabase(async (db) => {
    const tx = db.transaction(FEEDS_STORE, "readwrite");
    const store = tx.objectStore(FEEDS_STORE);
    const existing =
      (await requestToPromise<CachedDataFeed | undefined>(
        store.get(feedKey(viewerId, targetUserId)),
      )) ?? emptyFeed(viewerId, targetUserId);

    const batchIds = new Set(existing.batches.map((batch) => batch.id));
    for (const batch of page.batches) {
      if (!batchIds.has(batch.id)) {
        existing.batches.push(batch);
        batchIds.add(batch.id);
      }
    }

    const logIds = new Set(existing.logs.map((log) => log.id));
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
      ...page.batches.map((batch) => batch.created_at),
      ...page.logs.map((log) => log.created_at),
    );
    existing.since = Number.isFinite(latestCreatedAt)
      ? latestCreatedAt
      : existing.since;

    store.put(existing);
    await transactionDone(tx);
    return existing;
  });
}

export async function pruneCachedDataFeedDevices(
  viewerId: string,
  targetUserId: string,
  deviceIds: string[],
): Promise<CachedDataFeed> {
  return withDatabase(async (db) => {
    const tx = db.transaction(FEEDS_STORE, "readwrite");
    const store = tx.objectStore(FEEDS_STORE);
    const existing =
      (await requestToPromise<CachedDataFeed | undefined>(
        store.get(feedKey(viewerId, targetUserId)),
      )) ?? emptyFeed(viewerId, targetUserId);

    const allowedDeviceIds = new Set(deviceIds);
    const next: CachedDataFeed = {
      ...existing,
      batches: existing.batches.filter((batch) =>
        allowedDeviceIds.has(batch.device_id),
      ),
      logs: existing.logs.filter((log) => allowedDeviceIds.has(log.device_id)),
    };

    if (
      next.batches.length !== existing.batches.length ||
      next.logs.length !== existing.logs.length
    ) {
      store.put(next);
      await transactionDone(tx);
      return next;
    }
    await transactionDone(tx);
    return existing;
  });
}

export async function removeDeviceFromCachedDataFeed(
  viewerId: string,
  targetUserId: string,
  deviceId: string,
): Promise<CachedDataFeed> {
  return withDatabase(async (db) => {
    const tx = db.transaction(FEEDS_STORE, "readwrite");
    const store = tx.objectStore(FEEDS_STORE);
    const existing =
      (await requestToPromise<CachedDataFeed | undefined>(
        store.get(feedKey(viewerId, targetUserId)),
      )) ?? emptyFeed(viewerId, targetUserId);

    const next: CachedDataFeed = {
      ...existing,
      batches: existing.batches.filter((batch) => batch.device_id !== deviceId),
      logs: existing.logs.filter((log) => log.device_id !== deviceId),
    };

    if (
      next.batches.length !== existing.batches.length ||
      next.logs.length !== existing.logs.length
    ) {
      store.put(next);
      await transactionDone(tx);
      return next;
    }
    await transactionDone(tx);
    return existing;
  });
}

export async function clearDataCache(): Promise<void> {
  if (typeof indexedDB === "undefined") {
    return;
  }

  await withDatabase(async (db) => {
    const tx = db.transaction(
      [FEEDS_STORE, DECRYPTED_EVENTS_STORE, MATERIALIZED_BATCHES_STORE],
      "readwrite",
    );
    tx.objectStore(FEEDS_STORE).clear();
    tx.objectStore(DECRYPTED_EVENTS_STORE).clear();
    tx.objectStore(MATERIALIZED_BATCHES_STORE).clear();
    await transactionDone(tx);
  });
}

// ── Decrypted event cache functions ───────────────────────────────────────

export async function getUnmaterializedBatches(
  viewerId: string,
  batches: Batch[],
  cutoffTs: number,
): Promise<Batch[]> {
  if (batches.length === 0) return [];

  return withDatabase(async (db) => {
    const tx = db.transaction(MATERIALIZED_BATCHES_STORE, "readonly");
    const store = tx.objectStore(MATERIALIZED_BATCHES_STORE);
    const records = await getAllFromIndex<MaterializedBatchRecord>(
      store,
      "by_viewer",
      IDBKeyRange.only(viewerId),
    );
    await transactionDone(tx);

    const materializedIds = new Set(records.map((r) => r.batch_id));
    return batches.filter(
      (b) => !materializedIds.has(b.id) && b.created_at >= cutoffTs,
    );
  });
}

export async function writeMaterializedEvents(
  viewerId: string,
  batchId: string,
  deviceId: string,
  createdAt: number,
  events: FeedLog[],
): Promise<void> {
  return withDatabase(async (db) => {
    const tx = db.transaction(
      [DECRYPTED_EVENTS_STORE, MATERIALIZED_BATCHES_STORE],
      "readwrite",
    );
    const eventsStore = tx.objectStore(DECRYPTED_EVENTS_STORE);
    const batchesStore = tx.objectStore(MATERIALIZED_BATCHES_STORE);

    for (const event of events) {
      const stored: StoredDecryptedEvent = { ...event, viewer_id: viewerId };
      eventsStore.put(stored);
    }

    const record: MaterializedBatchRecord = {
      id: `${viewerId}:${batchId}`,
      viewer_id: viewerId,
      batch_id: batchId,
      device_id: deviceId,
      created_at: createdAt,
    };
    batchesStore.put(record);

    await transactionDone(tx);
  });
}

export async function queryDecryptedEvents(
  viewerId: string,
  { deviceId, startTs, endTs }: DecryptedEventQuery,
): Promise<FeedLog[]> {
  return withDatabase(async (db) => {
    const tx = db.transaction(DECRYPTED_EVENTS_STORE, "readonly");
    const store = tx.objectStore(DECRYPTED_EVENTS_STORE);

    let records: StoredDecryptedEvent[];

    if (startTs !== undefined && endTs !== undefined) {
      records = await getAllFromIndex<StoredDecryptedEvent>(
        store,
        "by_viewer_ts",
        IDBKeyRange.bound([viewerId, startTs], [viewerId, endTs]),
      );
    } else {
      records = await getAllFromIndex<StoredDecryptedEvent>(
        store,
        "by_viewer_ts",
        IDBKeyRange.bound([viewerId, 0], [viewerId, Infinity]),
      );
    }

    await transactionDone(tx);

    const filtered = deviceId
      ? records.filter((r) => r.device_id === deviceId)
      : records;

    const toBackfill: StoredDecryptedEvent[] = [];
    const updated = filtered.map((record) => {
      const hasDims =
        typeof record.image_w === "number" &&
        typeof record.image_h === "number";
      if (hasDims) return record;
      const image = record.data?.image;
      if (!(image instanceof Uint8Array)) return record;
      const dims = decodeWebpDimensions(image);
      if (!dims) return record;
      const next: StoredDecryptedEvent = {
        ...record,
        image_w: dims.width,
        image_h: dims.height,
      };
      toBackfill.push(next);
      return next;
    });

    if (toBackfill.length > 0) {
      void backfillDimensions(toBackfill).catch((err) =>
        console.warn("[data-cache] dimension backfill failed", err),
      );
    }

    return updated.map(({ viewer_id: _v, ...event }) => event as FeedLog);
  });
}

async function backfillDimensions(
  records: StoredDecryptedEvent[],
): Promise<void> {
  await withDatabase(async (db) => {
    const tx = db.transaction(DECRYPTED_EVENTS_STORE, "readwrite");
    const store = tx.objectStore(DECRYPTED_EVENTS_STORE);
    for (const record of records) {
      store.put(record);
    }
    await transactionDone(tx);
  });
}

export async function deleteDecryptedEventsForDevice(
  viewerId: string,
  deviceId: string,
): Promise<void> {
  return withDatabase(async (db) => {
    const tx = db.transaction(
      [DECRYPTED_EVENTS_STORE, MATERIALIZED_BATCHES_STORE],
      "readwrite",
    );

    await deleteByIndexCursor(
      tx.objectStore(DECRYPTED_EVENTS_STORE),
      "by_viewer_device",
      IDBKeyRange.only([viewerId, deviceId]),
    );

    await deleteByIndexCursor(
      tx.objectStore(MATERIALIZED_BATCHES_STORE),
      "by_viewer_device",
      IDBKeyRange.only([viewerId, deviceId]),
    );

    await transactionDone(tx);
  });
}

export async function pruneDecryptedEventsBefore(
  viewerId: string,
  cutoffTs: number,
): Promise<void> {
  return withDatabase(async (db) => {
    const tx = db.transaction(
      [DECRYPTED_EVENTS_STORE, MATERIALIZED_BATCHES_STORE],
      "readwrite",
    );

    await deleteByIndexCursor(
      tx.objectStore(DECRYPTED_EVENTS_STORE),
      "by_viewer_ts",
      IDBKeyRange.bound([viewerId, 0], [viewerId, cutoffTs], false, true),
    );

    const batchRecords = await getAllFromIndex<MaterializedBatchRecord>(
      tx.objectStore(MATERIALIZED_BATCHES_STORE),
      "by_viewer",
      IDBKeyRange.only(viewerId),
    );
    const batchesStore = tx.objectStore(MATERIALIZED_BATCHES_STORE);
    for (const record of batchRecords) {
      if (record.created_at < cutoffTs) {
        batchesStore.delete(record.id);
      }
    }

    await transactionDone(tx);
  });
}

export async function clearDecryptedCache(): Promise<void> {
  if (typeof indexedDB === "undefined") {
    return;
  }

  await withDatabase(async (db) => {
    const tx = db.transaction(
      [DECRYPTED_EVENTS_STORE, MATERIALIZED_BATCHES_STORE],
      "readwrite",
    );
    tx.objectStore(DECRYPTED_EVENTS_STORE).clear();
    tx.objectStore(MATERIALIZED_BATCHES_STORE).clear();
    await transactionDone(tx);
  });
}
