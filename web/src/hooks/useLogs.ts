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
}

export interface UseLogsResult {
  logs: FeedLog[] | undefined;
  hasMore: boolean | undefined;
  batchStats:
    | {
        decrypted: number;
        skipped: number;
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
    return selectedDeviceId
      ? feedEntries.filter((entry) => entry.device_id === selectedDeviceId)
      : feedEntries;
  }, [data, selectedDeviceId]);

  const visibleEntries = useMemo(
    () => filteredFeedEntries.slice(0, visibleCount),
    [filteredFeedEntries, visibleCount],
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

      try {
        const batchEntries = visibleEntries.flatMap((entry) =>
          entry.batch ? [entry.batch] : [],
        );
        const directLogs = visibleEntries.flatMap((entry) =>
          entry.log ? [toDirectLogEntry(entry.log)] : [],
        );

        let decrypted = 0;
        let skipped = activePrivateKey ? 0 : batchEntries.length;
        const batchLogs: FeedLog[] = [];

        if (activePrivateKey) {
          const results = await Promise.allSettled(
            batchEntries.map((batch) => {
              const cachedBatch = batchItemsCache.current.get(batch.id);
              if (cachedBatch) {
                return cachedBatch;
              }

              const promise = decryptAndFlattenBatch(
                batch,
                e2ee.unwrapEncryptedBatchKey,
              ).catch((decryptError) => {
                batchItemsCache.current.delete(batch.id);
                throw decryptError;
              });
              batchItemsCache.current.set(batch.id, promise);
              return promise;
            }),
          );

          for (const result of results) {
            if (result.status === "fulfilled") {
              batchLogs.push(...result.value);
              decrypted += 1;
            } else {
              skipped += 1;
              console.error("[logs] failed to decrypt batch", result.reason);
            }
          }
        }

        const merged = [...batchLogs, ...directLogs].sort(
          (a, b) => b.ts - a.ts,
        );

        if (!cancelled) {
          setLogs(merged);
          setBatchStats({ decrypted, skipped });
        }
      } finally {
        if (!cancelled) {
          setMaterializing(false);
        }
      }
    }

    void materializeVisibleItems();

    return () => {
      cancelled = true;
    };
  }, [data, visibleEntries, activePrivateKey, e2ee.unwrapEncryptedBatchKey]);

  return {
    logs,
    hasMore: data ? filteredFeedEntries.length > visibleCount : undefined,
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
