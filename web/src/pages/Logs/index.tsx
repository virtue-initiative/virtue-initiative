import { decode } from "@msgpack/msgpack";
import { useEffect, useMemo, useState } from "preact/hooks";
import { useLocation } from "preact-iso";
import { api, Batch, Device, Partner } from "../../api";
import { decryptBatch, decompressGzip } from "../../crypto";
import { useAuth } from "../../context/auth";
import { useE2EE } from "../../context/e2ee";
import { LogsGallery } from "./LogsGallery";
import { LogsList } from "./LogsList";
import { ImageLogItem, LogItem } from "./shared";
import "./style.css";

interface DeviceGroup {
  label: string;
  userId: string | null;
  devices: Device[];
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
  keyBytes: Uint8Array,
): Promise<LogItem[]> {
  const resp = await fetch(batch.url);
  if (!resp.ok) {
    throw new Error(`Fetch failed (${resp.status}) for ${batch.url}`);
  }

  const raw = new Uint8Array(await resp.arrayBuffer());
  if (raw.length < 13) {
    throw new Error(`Batch blob too short for AES-GCM payload: ${batch.url}`);
  }

  const keyMaterial = Uint8Array.from(keyBytes);
  const key = await crypto.subtle.importKey(
    "raw",
    keyMaterial,
    { name: "AES-GCM" },
    false,
    ["decrypt"],
  );
  const decrypted = await decryptBatch(key, raw);
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
            : batch.end,
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

export function Logs() {
  const { token, userId } = useAuth();
  const e2ee = useE2EE();
  const { path } = useLocation();

  const [deviceGroups, setDeviceGroups] = useState<DeviceGroup[]>([]);
  const [partners, setPartners] = useState<Partner[]>([]);
  const [selectedDevice, setSelectedDevice] = useState<string | null>(() =>
    new URLSearchParams(window.location.search).get("device_id"),
  );
  const [selectedUser, setSelectedUser] = useState<string | null>(() =>
    new URLSearchParams(window.location.search).get("user"),
  );
  const [sidebarLoading, setSidebarLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [items, setItems] = useState<LogItem[]>([]);
  const [nextCursor, setNextCursor] = useState<number | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [batchStats, setBatchStats] = useState({ decrypted: 0, skipped: 0 });

  const activeUserId = selectedUser ?? userId;
  const activeKeyBytes = activeUserId ? e2ee.getKeyBytes(activeUserId) : null;
  const activePartner =
    activeUserId && activeUserId !== userId
      ? (partners.find(
          (partner) =>
            partner.role === "invitee" && partner.partner.id === activeUserId,
        ) ?? null)
      : null;
  const missingPartnerKey = Boolean(
    activePartner && activePartner.permissions.view_data && !activeKeyBytes,
  );

  useEffect(() => {
    if (!token || !userId) return;
    setSidebarLoading(true);
    setLoadError(null);

    Promise.all([api.getDevices(token), api.getPartners(token)])
      .then(([devices, partners]) => {
        setPartners(partners);
        const labels = new Map<string, string>();
        labels.set(userId, "My devices");
        for (const partner of partners) {
          if (partner.partner.id) {
            labels.set(
              partner.partner.id,
              partner.partner.name ?? partner.partner.email,
            );
          }
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
    if (!token) return;
    setItems([]);
    setNextCursor(undefined);
    setBatchStats({ decrypted: 0, skipped: 0 });
    void doLoad(undefined, true);
  }, [token, selectedDevice, selectedUser, activeUserId, activeKeyBytes]);

  async function doLoad(cursor: number | undefined, reset: boolean) {
    if (!token) return;
    setLoading(true);
    setFetchError(null);

    try {
      const page = await api.getData(token, {
        user: selectedUser ?? undefined,
        device_id: selectedDevice ?? undefined,
        cursor,
        limit: 25,
      });

      const decryptedBatches = activeUserId
        ? await Promise.allSettled(
            page.batches.map(async (batch) => {
              const keyBytes =
                e2ee.getKeyBytesForTimestamp(activeUserId, batch.end) ??
                activeKeyBytes;
              if (!keyBytes) {
                throw new Error("No E2EE key available for batch timestamp");
              }
              return decryptAndFlattenBatch(batch, keyBytes);
            }),
          )
        : [];

      const batchItems: LogItem[] = [];
      let decrypted = 0;
      let skipped = activeUserId && activeKeyBytes ? 0 : page.batches.length;

      for (const result of decryptedBatches) {
        if (result.status === "fulfilled") {
          batchItems.push(...result.value);
          decrypted += 1;
        } else {
          skipped += 1;
          console.error("[logs] failed to decrypt batch", result.reason);
        }
      }

      const directLogs = page.logs.map((entry, index) => ({
        id: `${entry.device_id}:${entry.ts}:${index}`,
        taken_at: entry.ts,
        device_id: entry.device_id,
        kind: entry.type,
        image: toUint8Array(entry.data.image),
        metadata: toMetadata(entry.data),
        batch_status: "unknown" as const,
        source: "log" as const,
      }));

      const merged = [...batchItems, ...directLogs].sort(
        (a, b) => b.taken_at - a.taken_at,
      );

      setItems((prev) => (reset ? merged : [...prev, ...merged]));
      setNextCursor(page.next_cursor);
      setBatchStats((prev) => ({
        decrypted: (reset ? 0 : prev.decrypted) + decrypted,
        skipped: (reset ? 0 : prev.skipped) + skipped,
      }));
    } catch (err) {
      console.error("[logs] load failed:", err);
      setFetchError(err instanceof Error ? err.message : "Failed to load logs");
    } finally {
      setLoading(false);
    }
  }

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

  const title = selectedDevice
    ? `${selectedUser ? `${groupLabel(selectedUser)} — ` : ""}${deviceName(selectedDevice)}`
    : selectedUser
      ? `${groupLabel(selectedUser)}'s logs`
      : "All logs";

  const isGallery = path === "/logs/gallery";
  const galleryItems = items.filter((item): item is ImageLogItem =>
    Boolean(item.image),
  );

  return (
    <div class="logs-page">
      <div class="logs-layout">
        <aside class="logs-sidebar">
          {loadError && <p class="sidebar-loading">{loadError}</p>}
          {sidebarLoading && !loadError && (
            <p class="sidebar-loading">Loading…</p>
          )}
          {!sidebarLoading && deviceGroups.length === 0 && !loadError && (
            <p class="sidebar-loading">No devices yet.</p>
          )}
          {deviceGroups.map((group) => (
            <div class="sidebar-group" key={group.label}>
              <p class="sidebar-group-label">{group.label}</p>
              <ul class="device-list">
                <li>
                  <button
                    class={`device-btn${selectedUser === group.userId && selectedDevice === null ? " active" : ""}`}
                    onClick={() => select(group.userId, null)}
                    type="button"
                  >
                    All
                  </button>
                </li>
                {group.devices.map((device) => (
                  <li key={device.id}>
                    <button
                      class={`device-btn${selectedDevice === device.id ? " active" : ""}`}
                      onClick={() => select(group.userId, device.id)}
                      type="button"
                    >
                      <span
                        class={`dot ${device.status === "online" ? "dot-green" : "dot-gray"}`}
                      />
                      {device.name}
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </aside>

        <section class="logs-main">
          <div class="logs-header">
            <h1>{title}</h1>
            <div class="view-tabs">
              <a class={`view-tab${!isGallery ? " active" : ""}`} href="/logs">
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

          {fetchError && <p class="alert-error">{fetchError}</p>}
          {missingPartnerKey && (
            <div class="card settings-form">
              <p class="settings-hint">
                You are monitoring this person, but you do not have their
                decryption key yet, so encrypted screenshots and uploaded blocks
                cannot be shown. Ask the owner of these logs to click{" "}
                <strong>Confirm partner</strong> so the encrypted sharing key is
                attached to your partnership.
              </p>
            </div>
          )}
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
              hasMore={nextCursor !== undefined}
              onLoadMore={() => void doLoad(nextCursor, false)}
              deviceName={deviceName}
            />
          ) : (
            <LogsList
              items={items}
              loading={loading}
              hasMore={nextCursor !== undefined}
              onLoadMore={() => void doLoad(nextCursor, false)}
              deviceName={deviceName}
            />
          )}
        </section>
      </div>
    </div>
  );
}
