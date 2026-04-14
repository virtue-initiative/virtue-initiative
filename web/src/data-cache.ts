import { Batch, DataLog, DataPage } from "./api";

const DB_NAME = "virtue-data-cache";
const DB_VERSION = 1;
const FEEDS_STORE = "feeds";

export interface CachedDataFeed {
  key: string;
  viewer_id: string;
  target_user_id: string;
  since: number;
  batches: Batch[];
  logs: DataLog[];
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
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(FEEDS_STORE)) {
        db.createObjectStore(FEEDS_STORE, { keyPath: "key" });
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
    const tx = db.transaction(FEEDS_STORE, "readwrite");
    tx.objectStore(FEEDS_STORE).clear();
    await transactionDone(tx);
  });
}
