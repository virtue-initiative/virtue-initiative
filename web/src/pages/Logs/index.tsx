import { useEffect, useMemo, useState } from "preact/hooks";
import { useLocation } from "preact-iso";
import { Device, WatchingPartner } from "../../api";
import { useAuth } from "../../context/auth";
import { useDevices } from "../../hooks/useDevices";
import { useLogs } from "../../hooks/useLogs";
import { usePartners } from "../../hooks/usePartners";
import { LogsGallery } from "./LogsGallery";
import { LogsList } from "./LogsList";
import { FeedLog, getLogImage } from "./shared";
import "./style.css";

interface DeviceGroup {
  label: string;
  userId: string | null;
  devices: Device[];
}

interface UserLabel {
  id: string;
  label: string;
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

function MenuIcon() {
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
        d="M3.75 6.75h16.5M3.75 12h16.5m-16.5 5.25h16.5"
      />
    </svg>
  );
}

function CloseIcon() {
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
        d="M6 18 18 6M6 6l12 12"
      />
    </svg>
  );
}

export function Logs() {
  const { userId } = useAuth();
  const { path } = useLocation();
  const {
    devices,
    error: devicesError,
    isLoading: devicesLoading,
  } = useDevices();
  const {
    watching,
    error: partnersError,
    isLoading: partnersLoading,
  } = usePartners();

  const [selectedDevice, setSelectedDevice] = useState<string | null>(() =>
    new URLSearchParams(window.location.search).get("device_id"),
  );
  const [selectedUser, setSelectedUser] = useState<string | null>(() =>
    new URLSearchParams(window.location.search).get("user"),
  );
  const [galleryFullscreen, setGalleryFullscreen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);

  const {
    logs,
    hasMore,
    batchStats,
    error: logsError,
    isLoading: logsLoading,
    loadMore,
  } = useLogs({
    userId: selectedUser,
    deviceId: selectedDevice,
  });

  const deviceList = devices ?? [];
  const watchingList = watching ?? [];
  const loadError = devicesError ?? partnersError;
  const sidebarLoading = devicesLoading || partnersLoading;

  const { knownUsers, deviceGroups } = useMemo(() => {
    if (!userId) {
      return {
        knownUsers: [] as UserLabel[],
        deviceGroups: [] as DeviceGroup[],
      };
    }

    const labels = new Map<string, string>();
    labels.set(userId, "My devices");
    for (const partner of watchingList) {
      labels.set(partner.user.id, partner.user.name ?? partner.user.email);
    }

    const grouped = new Map<string, Device[]>();
    for (const device of deviceList) {
      const current = grouped.get(device.owner) ?? [];
      current.push(device);
      grouped.set(device.owner, current);
    }
    for (const ownerId of labels.keys()) {
      if (!grouped.has(ownerId)) {
        grouped.set(ownerId, []);
      }
    }

    return {
      knownUsers: Array.from(labels.entries()).map(([id, label]) => ({
        id,
        label,
      })),
      deviceGroups: Array.from(grouped.entries())
        .sort(([a], [b]) =>
          a === userId ? -1 : b === userId ? 1 : a.localeCompare(b),
        )
        .map(([owner, ownerDevices]) => ({
          label: labels.get(owner) ?? `${owner.slice(0, 8)}…`,
          userId: owner === userId ? null : owner,
          devices: ownerDevices,
        })),
    };
  }, [deviceList, userId, watchingList]);

  const activeGroup = useMemo(
    () => deviceGroups.find((group) => group.userId === selectedUser) ?? null,
    [deviceGroups, selectedUser],
  );
  const activeDevices = activeGroup?.devices ?? [];
  const activeDeviceIds = useMemo(
    () => new Set(activeDevices.map((device) => device.id)),
    [activeDevices],
  );

  useEffect(() => {
    if (sidebarLoading || loadError) {
      return;
    }

    if (
      selectedUser !== null &&
      !deviceGroups.some((group) => group.userId === selectedUser)
    ) {
      select(null, null);
      return;
    }

    if (selectedDevice && !activeDeviceIds.has(selectedDevice)) {
      select(selectedUser, null);
    }
  }, [
    activeDeviceIds,
    deviceGroups,
    loadError,
    selectedDevice,
    selectedUser,
    sidebarLoading,
  ]);

  const allDevices = useMemo(
    () => deviceGroups.flatMap((group) => group.devices),
    [deviceGroups],
  );

  const deviceName = (id: string) =>
    allDevices.find((device) => device.id === id)?.name ?? `${id.slice(0, 8)}…`;
  const groupLabel = (ownerId: string) =>
    knownUsers.find((entry) => entry.id === ownerId)?.label ??
    watchingList.find((partner) => partner.user.id === ownerId)?.user.name ??
    watchingList.find((partner) => partner.user.id === ownerId)?.user.email ??
    deviceGroups.find((group) => group.userId === ownerId)?.label ??
    `${ownerId.slice(0, 8)}…`;

  function select(user: string | null, device: string | null) {
    setSelectedUser(user);
    setSelectedDevice(device);
    setSidebarOpen(false);
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

  const title =
    sidebarLoading && selectedUser
      ? "Loading…"
      : selectedUser
        ? `${groupLabel(selectedUser)}'s logs`
        : "My logs";
  const isGallery = path === "/logs/gallery";
  const logsViewQuery = useMemo(() => {
    const qs = new URLSearchParams();
    if (selectedDevice) {
      qs.set("device_id", selectedDevice);
    }
    if (selectedUser) {
      qs.set("user", selectedUser);
    }
    const query = qs.toString();
    return query ? `?${query}` : "";
  }, [selectedDevice, selectedUser]);
  const items = logs ?? ([] as FeedLog[]);
  const galleryItems = items.filter((item) => getLogImage(item) !== undefined);

  useEffect(() => {
    if (!isGallery) {
      setGalleryFullscreen(false);
    }
  }, [isGallery]);

  return (
    <div
      class={`logs-page${isGallery && galleryFullscreen ? " logs-page--gallery-fullscreen" : ""}`}
    >
      <button
        class={`app-drawer-backdrop logs-sidebar-backdrop${sidebarOpen ? " is-open" : ""}`}
        type="button"
        aria-label="Close logs sidebar"
        onClick={() => setSidebarOpen(false)}
      />
      <div class="logs-layout">
        {!(isGallery && galleryFullscreen) && (
          <aside class={`logs-sidebar${sidebarOpen ? " is-open" : ""}`}>
            <div class="app-drawer-header logs-sidebar-header">
              <h2>Devices</h2>
              <button
                class="app-drawer-close logs-sidebar-close"
                type="button"
                aria-label="Close logs sidebar"
                onClick={() => setSidebarOpen(false)}
              >
                <CloseIcon />
              </button>
            </div>
            {loadError && (
              <p class="logs-sidebar-loading">{loadError.message}</p>
            )}
            {sidebarLoading && !loadError && (
              <p class="logs-sidebar-loading">Loading…</p>
            )}
            {!sidebarLoading && deviceGroups.length === 0 && !loadError && (
              <div class="logs-sidebar-group">
                <p class="logs-sidebar-group-label">My devices</p>
                <p class="logs-sidebar-loading">No devices</p>
              </div>
            )}
            {deviceGroups.map((group) => (
              <div class="logs-sidebar-group" key={group.label}>
                <button
                  class={`logs-device-button logs-device-button-group${selectedUser === group.userId && selectedDevice === null ? " is-active" : ""}`}
                  title={group.label}
                  onClick={() => select(group.userId, null)}
                  type="button"
                >
                  <span class="logs-status-dot logs-status-dot--placeholder" />
                  <span class="logs-device-button-label">{group.label}</span>
                </button>
                <ul class="logs-device-list">
                  {group.devices.map((device) => (
                    <li key={device.id}>
                      <button
                        class={`logs-device-button${selectedDevice === device.id ? " is-active" : ""}`}
                        onClick={() => select(group.userId, device.id)}
                        type="button"
                        title={device.name}
                      >
                        <span
                          class={`logs-status-dot ${device.status === "online" ? "logs-status-dot--online" : "logs-status-dot--offline"}`}
                        />
                        <span class="logs-device-button-label">
                          {device.name}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
                {group.devices.length === 0 && (
                  <p class="logs-sidebar-loading">No devices</p>
                )}
              </div>
            ))}
          </aside>
        )}

        <section class="logs-main">
          <div class="logs-header">
            <h1>{title}</h1>
            <div class="logs-header-actions">
              <button
                class="btn btn-ghost btn-sm logs-sidebar-toggle"
                type="button"
                onClick={() => setSidebarOpen(true)}
              >
                <MenuIcon />
                <span>Devices</span>
              </button>
              <div class="logs-header-view-controls">
                {isGallery && (
                  <button
                    class="btn btn-ghost btn-sm logs-fullscreen-btn"
                    type="button"
                    onClick={() => setGalleryFullscreen((prev) => !prev)}
                    aria-label={
                      galleryFullscreen ? "Exit fullscreen" : "Fullscreen"
                    }
                    title={galleryFullscreen ? "Exit fullscreen" : "Fullscreen"}
                  >
                    {galleryFullscreen ? (
                      <ExitFullscreenIcon />
                    ) : (
                      <ExpandIcon />
                    )}
                  </button>
                )}
                <div class="segmented-control logs-view-switcher">
                  <a
                    class={`segmented-control__item${!isGallery ? " is-active" : ""}`}
                    href={`/logs${logsViewQuery}`}
                  >
                    List
                  </a>
                  <a
                    class={`segmented-control__item${isGallery ? " is-active" : ""}`}
                    href={`/logs/gallery${logsViewQuery}`}
                  >
                    Gallery
                  </a>
                </div>
              </div>
            </div>
          </div>

          {logsError && <p class="alert-error">{logsError.message}</p>}
          {batchStats &&
            (batchStats.decrypted > 0 || batchStats.skipped > 0) && (
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
              loading={logsLoading}
              hasMore={hasMore ?? false}
              onLoadMore={loadMore}
              deviceName={deviceName}
              fullscreen={galleryFullscreen}
            />
          ) : (
            <LogsList
              items={items}
              loading={logsLoading}
              hasMore={hasMore ?? false}
              onLoadMore={loadMore}
              deviceName={deviceName}
            />
          )}
        </section>
      </div>
    </div>
  );
}
