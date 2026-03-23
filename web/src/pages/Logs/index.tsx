import { decode } from "@msgpack/msgpack";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { useLocation } from "preact-iso";
import { api, Batch, DataLog, Device, WatchingPartner } from "../../api";
import { decryptBatch, decompressGzip } from "../../crypto";
import {
  CachedDataFeed,
  loadCachedDataFeed,
  mergeDataPageIntoCache,
} from "../../data-cache";
import { useAuth } from "../../context/auth";
import { useE2EE } from "../../context/e2ee";
import { LogsGallery } from "./LogsGallery";
import { LogsList } from "./LogsList";
import { ImageLogItem, LogItem } from "./shared";
import "./style.css";

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

interface DeviceGroup {
  label: string;
  userId: string | null;
  devices: Device[];
}

function ExpandIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      strokeWidth={1.5}
      stroke="currentColor"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M3.75 3.75v4.5m0-4.5h4.5m-4.5 0L9 9M3.75 20.25v-4.5m0 4.5h4.5m-4.5 0L9 15M20.25 3.75h-4.5m4.5 0v4.5m0-4.5L15 9m5.25 11.25h-4.5m4.5 0v-4.5m0 4.5L15 15"
      />
    </svg>
  );
}

function ExitFullscreenIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      strokeWidth={1.5}
      stroke="currentColor"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M9 9V4.5M9 9H4.5M9 9 3.75 3.75M9 15v4.5M9 15H4.5M9 15l-5.25 5.25M15 9h4.5M15 9V4.5M15 9l5.25-5.25M15 15h4.5M15 15v4.5m0-4.5 5.25 5.25"
      />
    </svg>
  );
}

function toUint8Array(value: unknown): Uint8Array | undefined {
  if (!value) return undefined;
  if (value instanceof Uint8Array) return value;
  if (Array.isArray(value)) return new Uint8Array(value as number[]);
  if (typeof value === "string") {
    try {
      return Uint8Array.fromBase64(value);
    } catch {
      return undefined;
    }
  }
  return undefined;
}

function toMetadata(data: Record<string, unknown>) {
  return Object.entries(data)
    .filter(([key]) => key !== "image")
    .map(
      ([key, value]) =>
        [key, typeof value === "string" ? value : JSON.stringify(value)] as [
          string,
          string,
        ],
    );
}

function toMetadataEntries(value: unknown): [string, string][] {
  if (!Array.isArray(value)) return [];

  return value.flatMap((entry) => {
    if (!Array.isArray(entry) || entry.length < 2) return [];
    const [key, rawValue] = entry;
    if (typeof key !== "string") return [];
    return [
      [key, typeof rawValue === "string" ? rawValue : JSON.stringify(rawValue)],
    ];
  });
}

async function decryptAndFlattenBatch(
  batch: Batch,
  openBatchKey: (encryptedKey: string) => Promise<CryptoKey>,
): Promise<LogItem[]> {
  const resp = await fetch(batch.url);
  if (!resp.ok) {
    throw new Error(`Fetch failed (${resp.status}) for ${batch.url}`);
  }

  const raw = new Uint8Array(await resp.arrayBuffer());
  if (raw.length < 13) {
    throw new Error(`Batch blob too short for AES-GCM payload: ${batch.url}`);
  }

  const batchKey = await openBatchKey(batch.encrypted_key);
  const decrypted = await decryptBatch(batchKey, raw);
  const decompressed = await decompressGzip(decrypted);
  const decoded = decode(decompressed) as unknown;
  const record =
    decoded && typeof decoded === "object"
      ? (decoded as Record<string, unknown>)
      : {};
  const events = Array.isArray(record.events)
    ? (record.events as Record<string, unknown>[])
    : Array.isArray(record.items)
      ? (record.items as Record<string, unknown>[])
      : [];

  return events.map((event, index) => {
    const data =
      event.data && typeof event.data === "object"
        ? (event.data as Record<string, unknown>)
        : {};
    const metadata =
      "metadata" in event
        ? toMetadataEntries(event.metadata)
        : toMetadata(data);
    const image =
      toUint8Array("image" in event ? event.image : undefined) ??
      toUint8Array(data.image);

    return {
      id: typeof event.id === "string" ? event.id : `${batch.id}:${index}`,
      taken_at:
        typeof event.ts === "number"
          ? event.ts
          : typeof event.taken_at === "number"
            ? event.taken_at
            : batch.end_time,
      device_id: batch.device_id,
      kind:
        typeof event.type === "string"
          ? event.type
          : typeof event.kind === "string"
            ? event.kind
            : "unknown",
      image,
      metadata,
      batch_status: "unknown" as const,
      source: "batch" as const,
    };
  });
}

function toDirectLogItem(entry: DataLog): LogItem {
  return {
    id: entry.id,
    taken_at: entry.ts,
    device_id: entry.device_id,
    kind: entry.type,
    image: toUint8Array(entry.data.image),
    metadata: toMetadata(entry.data),
    batch_status: "unknown" as const,
    source: "log" as const,
  };
}

export function Logs() {
  const { token, userId } = useAuth();
  const e2ee = useE2EE();
  const { path } = useLocation();

  const [deviceGroups, setDeviceGroups] = useState<DeviceGroup[]>([]);
  const [partners, setPartners] = useState<WatchingPartner[]>([]);
  const [selectedDevice, setSelectedDevice] = useState<string | null>(() =>
    new URLSearchParams(window.location.search).get("device_id"),
  );
  const [selectedUser, setSelectedUser] = useState<string | null>(() =>
    new URLSearchParams(window.location.search).get("user"),
  );
  const [sidebarLoading, setSidebarLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [cachedFeed, setCachedFeed] = useState<CachedDataFeed | null>(null);
  const [items, setItems] = useState<LogItem[]>([]);
  const [visibleCount, setVisibleCount] = useState(VISIBLE_PAGE_SIZE);
  const [syncing, setSyncing] = useState(false);
  const [materializing, setMaterializing] = useState(false);
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [batchStats, setBatchStats] = useState({ decrypted: 0, skipped: 0 });
  const [galleryFullscreen, setGalleryFullscreen] = useState(false);
  const batchItemsCache = useRef(new Map<string, Promise<LogItem[]>>());

  const activeUserId = selectedUser ?? userId;
  const activePrivateKey = e2ee.privateKey;

  useEffect(() => {
    if (!token || !userId) return;
    setSidebarLoading(true);
    setLoadError(null);

    Promise.all([api.getDevices(token), api.getPartners(token)])
      .then(([devices, partners]) => {
        setPartners(partners.watching);
        const labels = new Map<string, string>();
        labels.set(userId, "My devices");
        for (const partner of partners.watching) {
          labels.set(partner.user.id, partner.user.name ?? partner.user.email);
        }

        const grouped = new Map<string, Device[]>();
        for (const device of devices) {
          const current = grouped.get(device.owner) ?? [];
          current.push(device);
          grouped.set(device.owner, current);
        }

        const groups = Array.from(grouped.entries())
          .sort(([a], [b]) =>
            a === userId ? -1 : b === userId ? 1 : a.localeCompare(b),
          )
          .map(([owner, ownerDevices]) => ({
            label: labels.get(owner) ?? `${owner.slice(0, 8)}…`,
            userId: owner === userId ? null : owner,
            devices: ownerDevices,
          }));

        setDeviceGroups(groups);
      })
      .catch((err) => {
        setLoadError(
          err instanceof Error ? err.message : "Failed to load devices",
        );
      })
      .finally(() => {
        setSidebarLoading(false);
      });
  }, [token, userId]);

  useEffect(() => {
    batchItemsCache.current.clear();
  }, [activeUserId, activePrivateKey]);

  useEffect(() => {
    if (!token || !userId || !activeUserId || !e2ee.ready) return;

    let cancelled = false;
    const targetUserId = activeUserId;
    setFetchError(null);
    setCachedFeed(null);
    setItems([]);
    setBatchStats({ decrypted: 0, skipped: 0 });
    setVisibleCount(VISIBLE_PAGE_SIZE);
    setSyncing(true);

    async function loadAndSync() {
      try {
        const cached = await loadCachedDataFeed(userId, targetUserId);
        if (cancelled) return;
        setCachedFeed(cached);

        let since = cached.since;
        while (!cancelled) {
          const page = await api.getData(token, {
            user: selectedUser ?? undefined,
            since,
            limit: SYNC_PAGE_SIZE,
          });
          if (cancelled) return;

          if (page.batches.length === 0 && page.logs.length === 0) {
            break;
          }

          const updated = await mergeDataPageIntoCache(
            userId,
            targetUserId,
            page,
          );
          if (cancelled) return;
          since = updated.since;

          if (page.next_since === undefined) {
            break;
          }
        }

        if (cancelled) return;
        setCachedFeed(await loadCachedDataFeed(userId, targetUserId));
      } catch (err) {
        console.error("[logs] sync failed:", err);
        if (!cancelled) {
          setCachedFeed(await loadCachedDataFeed(userId, targetUserId));
          setFetchError(
            err instanceof Error ? err.message : "Failed to sync logs",
          );
        }
      } finally {
        if (!cancelled) {
          setSyncing(false);
        }
      }
    }

    void loadAndSync();

    return () => {
      cancelled = true;
    };
  }, [token, userId, activeUserId, selectedUser, e2ee.ready]);

  const filteredFeedEntries = useMemo<FeedEntry[]>(() => {
    if (!cachedFeed) {
      return [];
    }

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
    ].sort((a, b) => b.created_at - a.created_at);

    return selectedDevice
      ? combined.filter((entry) => entry.device_id === selectedDevice)
      : combined;
  }, [cachedFeed, selectedDevice]);

  const visibleEntries = useMemo(
    () => filteredFeedEntries.slice(0, visibleCount),
    [filteredFeedEntries, visibleCount],
  );

  useEffect(() => {
    let cancelled = false;

    async function materializeVisibleItems() {
      if (!cachedFeed) {
        setItems([]);
        setBatchStats({ decrypted: 0, skipped: 0 });
        setMaterializing(false);
        return;
      }

      setMaterializing(true);

      try {
        const batchEntries = visibleEntries.flatMap((entry) =>
          entry.batch ? [entry.batch] : [],
        );
        const directLogs = visibleEntries.flatMap((entry) =>
          entry.log ? [toDirectLogItem(entry.log)] : [],
        );

        let decrypted = 0;
        let skipped = activePrivateKey ? 0 : batchEntries.length;
        const batchItems: LogItem[] = [];

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
              ).catch((error) => {
                batchItemsCache.current.delete(batch.id);
                throw error;
              });
              batchItemsCache.current.set(batch.id, promise);
              return promise;
            }),
          );

          for (const result of results) {
            if (result.status === "fulfilled") {
              batchItems.push(...result.value);
              decrypted += 1;
            } else {
              skipped += 1;
              console.error("[logs] failed to decrypt batch", result.reason);
            }
          }
        }

        const merged = [...batchItems, ...directLogs].sort(
          (a, b) => b.taken_at - a.taken_at,
        );

        if (!cancelled) {
          setItems(merged);
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
  }, [
    cachedFeed,
    visibleEntries,
    activePrivateKey,
    e2ee.unwrapEncryptedBatchKey,
  ]);

  const allDevices = useMemo(
    () => deviceGroups.flatMap((group) => group.devices),
    [deviceGroups],
  );

  const deviceName = (id: string) =>
    allDevices.find((device) => device.id === id)?.name ?? `${id.slice(0, 8)}…`;
  const groupLabel = (ownerId: string) =>
    deviceGroups.find((group) => group.userId === ownerId)?.label ??
    `${ownerId.slice(0, 8)}…`;

  function select(user: string | null, device: string | null) {
    setSelectedUser(user);
    setSelectedDevice(device);
    setVisibleCount(VISIBLE_PAGE_SIZE);
    const qs = new URLSearchParams(window.location.search);
    if (device) qs.set("device_id", device);
    else qs.delete("device_id");
    if (user) qs.set("user", user);
    else qs.delete("user");
    const query = qs.toString();
    window.history.replaceState(
      null,
      "",
      `${window.location.pathname}${query ? `?${query}` : ""}`,
    );
  }

  const title = selectedUser ? `${groupLabel(selectedUser)}'s logs` : "My logs";

  const isGallery = path === "/logs/gallery";
  const galleryItems = items.filter((item): item is ImageLogItem =>
    Boolean(item.image),
  );
  const loading = syncing || materializing;
  const hasMore = filteredFeedEntries.length > visibleCount;
  useEffect(() => {
    if (!isGallery) {
      setGalleryFullscreen(false);
    }
  }, [isGallery]);

  return (
    <div
      class={`logs-page${isGallery && galleryFullscreen ? " logs-page--gallery-fullscreen" : ""}`}
    >
      <div class="logs-layout">
        {!(isGallery && galleryFullscreen) && (
          <aside class="logs-sidebar">
            {loadError && <p class="sidebar-loading">{loadError}</p>}
            {sidebarLoading && !loadError && (
              <p class="sidebar-loading">Loading…</p>
            )}
            {!sidebarLoading && deviceGroups.length === 0 && !loadError && (
              <div class="sidebar-group">
                <p class="sidebar-group-label">My devices</p>
                <p class="sidebar-loading">No devices registered yet.</p>
              </div>
            )}
            {deviceGroups.map((group) => (
              <div class="sidebar-group" key={group.label}>
                <p class="sidebar-group-label" title={group.label}>
                  {group.label}
                </p>
                <ul class="device-list">
                  <li>
                    <button
                      class={`device-btn${selectedUser === group.userId && selectedDevice === null ? " active" : ""}`}
                      onClick={() => select(group.userId, null)}
                      type="button"
                    >
                      <span class="device-btn-label">All</span>
                    </button>
                  </li>
                  {group.devices.map((device) => (
                    <li key={device.id}>
                      <button
                        class={`device-btn${selectedDevice === device.id ? " active" : ""}`}
                        onClick={() => select(group.userId, device.id)}
                        type="button"
                        title={device.name}
                      >
                        <span
                          class={`dot ${device.status === "online" ? "dot-green" : "dot-gray"}`}
                        />
                        <span class="device-btn-label">{device.name}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </aside>
        )}

        <section class="logs-main">
          <div class="logs-header">
            <h1>{title}</h1>
            <div class="logs-header-actions">
              <button
                class={`btn btn-ghost btn-sm logs-fullscreen-btn${isGallery ? "" : " logs-fullscreen-btn--hidden"}`}
                type="button"
                onClick={() => setGalleryFullscreen((prev) => !prev)}
                aria-label={
                  galleryFullscreen ? "Exit fullscreen" : "Fullscreen"
                }
                title={galleryFullscreen ? "Exit fullscreen" : "Fullscreen"}
                disabled={!isGallery}
                tabIndex={isGallery ? 0 : -1}
              >
                {galleryFullscreen ? <ExitFullscreenIcon /> : <ExpandIcon />}
              </button>
              <div class="view-tabs">
                <a
                  class={`view-tab${!isGallery ? " active" : ""}`}
                  href="/logs"
                >
                  List
                </a>
                <a
                  class={`view-tab${isGallery ? " active" : ""}`}
                  href="/logs/gallery"
                >
                  Gallery
                </a>
              </div>
            </div>
          </div>

          {fetchError && <p class="alert-error">{fetchError}</p>}
          {(batchStats.decrypted > 0 || batchStats.skipped > 0) && (
            <p class="logs-summary">
              {batchStats.decrypted} block
              {batchStats.decrypted === 1 ? "" : "s"} decrypted
              {batchStats.skipped > 0 &&
                `, ${batchStats.skipped} block${batchStats.skipped === 1 ? "" : "s"} unavailable`}
            </p>
          )}

          {isGallery ? (
            <LogsGallery
              items={galleryItems}
              loading={loading}
              hasMore={hasMore}
              onLoadMore={() =>
                setVisibleCount((prev) => prev + VISIBLE_PAGE_SIZE)
              }
              deviceName={deviceName}
              fullscreen={galleryFullscreen}
            />
          ) : (
            <LogsList
              items={items}
              loading={loading}
              hasMore={hasMore}
              onLoadMore={() =>
                setVisibleCount((prev) => prev + VISIBLE_PAGE_SIZE)
              }
              deviceName={deviceName}
            />
          )}
        </section>
      </div>
    </div>
  );
}
