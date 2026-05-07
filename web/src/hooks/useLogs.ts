import { decode } from "@msgpack/msgpack";
import useSWR from "swr";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { api, Batch, DataLog, isToastHandledError } from "../api";
import { decryptBatch, decompressGzip } from "../crypto";
import {
  CachedDataFeed,
  loadCachedDataFeed,
  mergeDataPageIntoCache,
  pruneCachedDataFeedDevices,
} from "../data-cache";
import { useAuth } from "../context/auth";
import { useE2EE } from "../context/e2ee";
import { FeedLog, toUint8Array } from "../pages/Logs/shared";
import { useDevices } from "./useDevices";
import { swrKeys } from "./swr-keys";

const SYNC_PAGE_SIZE = 250;
const VISIBLE_PAGE_SIZE = 25;
const DECRYPT_CONCURRENCY = 5;

interface FeedEntry {
  key: string;
  created_at: number;
  device_id: string;
  kind: "batch" | "log";
  batch?: Batch;
  log?: DataLog;
}

interface SyncedLogFeed {
  cachedFeed: CachedDataFeed;
  feedEntries: FeedEntry[];
}

export interface UseLogsOptions {
  userId: string | null;
  deviceId: string | null;
  startTime?: number;
  endTime?: number;
}

export interface UseLogsResult {
  logs: FeedLog[] | undefined;
  hasMore: boolean | undefined;
  batchStats:
    | {
        decrypted: number;
        skipped: number;
        total: number;
      }
    | undefined;
  error: Error | undefined;
  isLoading: boolean;
  loadMore: () => void;
  refresh: () => Promise<void>;
}

function toDirectLogEntry(entry: DataLog): FeedLog {
  return {
    ...entry,
    batch_status: "unknown" as const,
    source: "log" as const,
  };
}

async function decryptAndFlattenBatch(
  batch: Batch,
  openBatchKey: (encryptedKey: string) => Promise<CryptoKey>,
): Promise<FeedLog[]> {
  const response = await fetch(batch.url);
  if (!response.ok) {
    throw new Error(`Fetch failed (${response.status}) for ${batch.url}`);
  }

  const raw = new Uint8Array(await response.arrayBuffer());
  if (raw.length < 13) {
    throw new Error(`Batch blob too short for AES-GCM payload: ${batch.url}`);
  }

  const batchKey = await openBatchKey(batch.encrypted_key);
  const decrypted = await decryptBatch(batchKey, raw);
  const decompressed = await decompressGzip(decrypted);
  const decoded = decode(decompressed) as unknown;
  const eventBytes = Array.isArray(decoded) ? decoded : [];

  return eventBytes.map((encodedEvent, index) => {
    const rawEvent = toUint8Array(encodedEvent);
    if (!rawEvent) {
      throw new Error(`Batch event ${index} is not a byte array`);
    }

    const event = decode(rawEvent) as Record<string, unknown>;
    const data =
      event.data && typeof event.data === "object"
        ? (event.data as Record<string, unknown>)
        : {};

    if (Array.isArray(data.image)) {
      data.image = new Uint8Array(data.image as number[]);
    }

    return {
      id: typeof event.id === "string" ? event.id : `${batch.id}:${index}`,
      device_id: batch.device_id,
      ts: typeof event.ts === "number" ? event.ts : batch.end_time,
      type: typeof event.type === "string" ? event.type : "unknown",
      data,
      created_at: batch.created_at,
      risk: typeof event.risk === "number" ? event.risk : undefined,
      batch_status: "unknown" as const,
      source: "batch" as const,
    };
  });
}

function buildFilteredFeedEntries(
  cachedFeed: CachedDataFeed,
  activeDeviceIds: Set<string>,
): FeedEntry[] {
  const combined = [
    ...cachedFeed.batches.map((batch) => ({
      key: batch.id,
      created_at: batch.created_at,
      device_id: batch.device_id,
      kind: "batch" as const,
      batch,
    })),
    ...cachedFeed.logs.map((log) => ({
      key: log.id,
      created_at: log.created_at,
      device_id: log.device_id,
      kind: "log" as const,
      log,
    })),
  ]
    .filter((entry) => activeDeviceIds.has(entry.device_id))
    .sort((a, b) => b.created_at - a.created_at);
  return combined;
}

export function useLogs({
  userId: selectedUserId,
  deviceId: selectedDeviceId,
  startTime,
  endTime,
}: UseLogsOptions): UseLogsResult {
  const { token, userId: viewerUserId } = useAuth();
  const {
    devices,
    error: devicesError,
    isLoading: devicesLoading,
  } = useDevices();
  const e2ee = useE2EE();
  const [visibleCount, setVisibleCount] = useState(VISIBLE_PAGE_SIZE);
  const [logs, setLogs] = useState<FeedLog[]>();
  const [batchStats, setBatchStats] = useState<
    | {
        decrypted: number;
        skipped: number;
        total: number;
      }
    | undefined
  >();
  const [materializing, setMaterializing] = useState(false);
  const batchItemsCache = useRef(new Map<string, Promise<FeedLog[]>>());

  const activeTargetUserId = selectedUserId ?? viewerUserId;
  const activePrivateKey = e2ee.privateKey;
  const activeDevices = useMemo(
    () =>
      (devices ?? []).filter((device) => device.owner === activeTargetUserId),
    [devices, activeTargetUserId],
  );
  const activeDeviceIdList = useMemo(
    () => activeDevices.map((device) => device.id).sort(),
    [activeDevices],
  );
  const activeDeviceIds = useMemo(
    () => new Set(activeDeviceIdList),
    [activeDeviceIdList],
  );
  const activeDeviceIdsKey = activeDeviceIdList.join(",");

  useEffect(() => {
    setVisibleCount(VISIBLE_PAGE_SIZE);
  }, [activeTargetUserId, selectedDeviceId, activeDeviceIdsKey]);

  useEffect(() => {
    batchItemsCache.current.clear();
  }, [activeTargetUserId, activePrivateKey]);

  const key =
    token && viewerUserId && activeTargetUserId && !devicesLoading && e2ee.ready
      ? swrKeys.logs(
          token,
          viewerUserId,
          activeTargetUserId,
          activeDeviceIdsKey,
        )
      : null;

  const {
    data,
    error,
    isLoading: feedLoading,
    mutate,
  } = useSWR<SyncedLogFeed, Error>(key, async () => {
    if (!token || !viewerUserId || !activeTargetUserId) {
      throw new Error(
        "Log data is not available without an authenticated user.",
      );
    }

    let cachedFeed = await loadCachedDataFeed(viewerUserId, activeTargetUserId);
    cachedFeed = await pruneCachedDataFeedDevices(
      viewerUserId,
      activeTargetUserId,
      activeDeviceIdList,
    );

    let since = cachedFeed.since;
    while (true) {
      const page = await api.getData(token, {
        user:
          activeTargetUserId === viewerUserId ? undefined : activeTargetUserId,
        since,
        limit: SYNC_PAGE_SIZE,
      });

      if (page.batches.length === 0 && page.logs.length === 0) {
        break;
      }

      const updated = await mergeDataPageIntoCache(
        viewerUserId,
        activeTargetUserId,
        page,
      );
      since = updated.since;

      if (page.next_since === undefined) {
        break;
      }
    }

    cachedFeed = await loadCachedDataFeed(viewerUserId, activeTargetUserId);
    cachedFeed = await pruneCachedDataFeedDevices(
      viewerUserId,
      activeTargetUserId,
      activeDeviceIdList,
    );

    return {
      cachedFeed,
      feedEntries: buildFilteredFeedEntries(cachedFeed, activeDeviceIds),
    };
  });

  const filteredFeedEntries = useMemo(() => {
    const feedEntries = data?.feedEntries ?? [];
    let filtered = selectedDeviceId
      ? feedEntries.filter((entry) => entry.device_id === selectedDeviceId)
      : feedEntries;

    if (startTime !== undefined && endTime !== undefined) {
      filtered = filtered.filter((entry) => {
        if (entry.batch) {
          return entry.batch.start_time <= endTime && entry.batch.end_time >= startTime;
        }
        if (entry.log) {
          return entry.log.ts >= startTime && entry.log.ts <= endTime;
        }
        return false;
      });
    }

    return filtered;
  }, [data, selectedDeviceId, startTime, endTime]);

  const dateRangeActive = startTime !== undefined && endTime !== undefined;

  const visibleEntries = useMemo(
    () => dateRangeActive ? filteredFeedEntries : filteredFeedEntries.slice(0, visibleCount),
    [filteredFeedEntries, visibleCount, dateRangeActive],
  );

  useEffect(() => {
    let cancelled = false;

    async function materializeVisibleItems() {
      if (!data) {
        setLogs(undefined);
        setBatchStats(undefined);
        setMaterializing(false);
        return;
      }

      setMaterializing(true);

      const batchEntries = visibleEntries.flatMap((entry) =>
        entry.batch ? [entry.batch] : [],
      );
      const directLogs = visibleEntries.flatMap((entry) =>
        entry.log ? [toDirectLogEntry(entry.log)] : [],
      );

      // Show direct logs immediately
      if (!cancelled) {
        setLogs([...directLogs].sort((a, b) => b.ts - a.ts));
        setBatchStats({
          decrypted: 0,
          skipped: activePrivateKey ? 0 : batchEntries.length,
          total: batchEntries.length,
        });
      }

      if (!activePrivateKey || batchEntries.length === 0) {
        if (!cancelled) setMaterializing(false);
        return;
      }

      let decrypted = 0;
      let skipped = 0;
      let completed = 0;
      const accumBatchLogs: FeedLog[] = [];
      const queue = [...batchEntries]; // newest-first (already sorted by created_at desc)

      async function worker() {
        while (queue.length > 0) {
          if (cancelled) return;
          const batch = queue.shift()!;
          try {
            let promise = batchItemsCache.current.get(batch.id);
            if (!promise) {
              promise = decryptAndFlattenBatch(
                batch,
                e2ee.unwrapEncryptedBatchKey,
              ).catch((err) => {
                batchItemsCache.current.delete(batch.id);
                throw err;
              });
              batchItemsCache.current.set(batch.id, promise);
            }
            const batchLogs = await promise;
            if (cancelled) return;
            accumBatchLogs.push(...batchLogs);
            decrypted++;
          } catch (err) {
            if (cancelled) return;
            console.error("[logs] failed to decrypt batch", err);
            skipped++;
          }
          completed++;
          setLogs(
            [...accumBatchLogs, ...directLogs].sort((a, b) => b.ts - a.ts),
          );
          setBatchStats({ decrypted, skipped, total: batchEntries.length });
          if (completed === batchEntries.length) setMaterializing(false);
        }
      }

      await Promise.all(
        Array.from(
          { length: Math.min(DECRYPT_CONCURRENCY, batchEntries.length) },
          worker,
        ),
      );
    }

    void materializeVisibleItems();

    return () => {
      cancelled = true;
    };
  }, [data, visibleEntries, activePrivateKey, e2ee.unwrapEncryptedBatchKey]);

  return {
    logs,
    hasMore: data ? (!dateRangeActive && filteredFeedEntries.length > visibleCount) : undefined,
    batchStats,
    error: [devicesError, error].find(
      (candidate) => candidate && !isToastHandledError(candidate),
    ),
    isLoading:
      (Boolean(token && viewerUserId) &&
        (devicesLoading || !e2ee.ready || feedLoading || materializing)) ||
      false,
    loadMore: () => {
      setVisibleCount((previous) => previous + VISIBLE_PAGE_SIZE);
    },
    refresh: async () => {
      await mutate();
    },
  };
}
