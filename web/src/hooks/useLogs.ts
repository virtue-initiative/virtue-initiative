import useSWR from "swr";
import { useEffect, useMemo, useState } from "preact/hooks";
import { api, Batch, DataLog, isToastHandledError } from "../api";
import { decryptAndFlattenBatch } from "../batch-materializer";
import {
  CachedDataFeed,
  loadCachedDataFeed,
  mergeDataPageIntoCache,
  pruneCachedDataFeedDevices,
  getUnmaterializedBatches,
  queryDecryptedEvents,
  writeMaterializedEvents,
} from "../data-cache";
import { useAuth } from "../context/auth";
import { useE2EE } from "../context/e2ee";
import { FeedLog } from "../pages/Logs/shared";
import { useDevices } from "./useDevices";
import { useDecryptedEventSync } from "./useDecryptedEventSync";
import { swrKeys } from "./swr-keys";

const SYNC_PAGE_SIZE = 250;
const VISIBLE_PAGE_SIZE = 25;
const DECRYPT_CONCURRENCY = 5;
const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;

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
    | { decrypted: number; skipped: number; total: number }
    | undefined
  >();
  const [materializing, setMaterializing] = useState(false);

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

  // Background sync: materializes all batches to IDB newest-first
  useDecryptedEventSync({
    viewerId: viewerUserId,
    batches: data?.cachedFeed.batches ?? [],
    unwrapEncryptedBatchKey: e2ee.unwrapEncryptedBatchKey,
    privateKeyReady: e2ee.ready && !!activePrivateKey,
  });

  const filteredFeedEntries = useMemo(() => {
    const feedEntries = data?.feedEntries ?? [];
    let filtered = selectedDeviceId
      ? feedEntries.filter((entry) => entry.device_id === selectedDeviceId)
      : feedEntries;

    if (startTime !== undefined && endTime !== undefined) {
      filtered = filtered.filter((entry) => {
        if (entry.batch) {
          return (
            entry.batch.start_time <= endTime &&
            entry.batch.end_time >= startTime
          );
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
    () =>
      dateRangeActive
        ? filteredFeedEntries
        : filteredFeedEntries.slice(0, visibleCount),
    [filteredFeedEntries, visibleCount, dateRangeActive],
  );

  useEffect(() => {
    if (!data || !viewerUserId) {
      setLogs(undefined);
      setBatchStats(undefined);
      setMaterializing(false);
      return;
    }

    let cancelled = false;

    async function loadLogs() {
      setMaterializing(true);

      // Query IDB for already-materialized events in the visible range
      const cachedEvents = await queryDecryptedEvents(viewerUserId!, {
        deviceId: selectedDeviceId ?? undefined,
        startTs: startTime,
        endTs: endTime,
      });

      if (cancelled) return;

      // Direct (unencrypted) logs from the feed
      const directLogs = visibleEntries.flatMap((entry) =>
        entry.log ? [toDirectLogEntry(entry.log)] : [],
      );

      // Show cached + direct logs immediately
      setLogs(
        [...cachedEvents, ...directLogs].sort((a, b) => b.ts - a.ts),
      );

      // Determine which visible batches still need on-demand decryption
      const cutoff = Date.now() - THIRTY_DAYS_MS;
      const visibleBatches = visibleEntries.flatMap((entry) =>
        entry.batch ? [entry.batch] : [],
      );

      let unmaterialized: Batch[];
      try {
        unmaterialized = await getUnmaterializedBatches(
          viewerUserId!,
          visibleBatches,
          cutoff,
        );
      } catch (err) {
        console.warn("[logs] failed to check materialized batches", err);
        unmaterialized = visibleBatches;
      }

      if (cancelled) return;

      const alreadyDecrypted = visibleBatches.length - unmaterialized.length;
      setBatchStats({
        decrypted: alreadyDecrypted,
        skipped: 0,
        total: visibleBatches.length,
      });

      if (!activePrivateKey || unmaterialized.length === 0) {
        setMaterializing(false);
        return;
      }

      let decrypted = alreadyDecrypted;
      let skipped = 0;
      let completed = 0;
      const accumLogs: FeedLog[] = [...cachedEvents, ...directLogs];
      const queue = [...unmaterialized];

      async function worker() {
        while (queue.length > 0) {
          if (cancelled) return;
          const batch = queue.shift()!;
          try {
            const batchLogs = await decryptAndFlattenBatch(
              batch,
              e2ee.unwrapEncryptedBatchKey,
            );
            if (cancelled) return;
            accumLogs.push(...batchLogs);
            decrypted++;
            // Write to IDB so future visits load instantly
            writeMaterializedEvents(
              viewerUserId!,
              batch.id,
              batch.device_id,
              batch.created_at,
              batchLogs,
            ).catch((err) =>
              console.warn("[logs] failed to cache batch", err),
            );
          } catch (err) {
            if (cancelled) return;
            console.error("[logs] failed to decrypt batch", err);
            skipped++;
          }
          completed++;
          setLogs([...accumLogs].sort((a, b) => b.ts - a.ts));
          setBatchStats({ decrypted, skipped, total: visibleBatches.length });
          if (completed === unmaterialized.length) setMaterializing(false);
        }
      }

      await Promise.all(
        Array.from(
          { length: Math.min(DECRYPT_CONCURRENCY, unmaterialized.length) },
          worker,
        ),
      );
    }

    void loadLogs();

    return () => {
      cancelled = true;
    };
  }, [
    data,
    viewerUserId,
    selectedDeviceId,
    startTime,
    endTime,
    visibleEntries,
    activePrivateKey,
    e2ee.unwrapEncryptedBatchKey,
  ]);

  return {
    logs,
    hasMore: data
      ? !dateRangeActive && filteredFeedEntries.length > visibleCount
      : undefined,
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
