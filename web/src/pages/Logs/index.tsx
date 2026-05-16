import { useEffect, useMemo, useState, useCallback } from "preact/hooks";
import { useLocation } from "preact-iso";
import { Device } from "../../api";
import { useAuth } from "../../context/auth";
import { useDevices } from "../../hooks/useDevices";
import { useLogs } from "../../hooks/useLogs";
import { usePartners } from "../../hooks/usePartners";
import { LogsGallery } from "./LogsGallery";
import { LogsList } from "./LogsList";
import {
  getRiskRating,
  type RiskRating,
} from "@virtueinitiative/shared-web/risk";
import { getLogImage, humanizeLogType, LOG_TYPES } from "./shared";
import "./style.css";
import { useUrlState } from "../../hooks/useUrlState";
import {
  Alert,
  Button,
  Field,
  IconButton,
  Select,
} from "@virtueinitiative/shared-web";

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

function ChevronLeftIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      strokeWidth={2}
      stroke="currentColor"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M15.75 19.5 8.25 12l7.5-7.5"
      />
    </svg>
  );
}

function ChevronRightIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      strokeWidth={2}
      stroke="currentColor"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="m8.25 4.5 7.5 7.5-7.5 7.5"
      />
    </svg>
  );
}

const dayLabelFormatter = new Intl.DateTimeFormat(undefined, {
  weekday: "long",
  month: "short",
  day: "numeric",
  year: "numeric",
});

function getDayBounds(offset: number): { startMs: number; endMs: number } {
  const day = new Date();
  day.setHours(0, 0, 0, 0);
  day.setDate(day.getDate() + offset);
  const end = new Date(day);
  end.setHours(23, 59, 59, 999);
  return { startMs: day.getTime(), endMs: end.getTime() };
}

function formatDayLabel(startMs: number): string {
  return dayLabelFormatter.format(new Date(startMs));
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

  const [selectedDevice, setSelectedDevice] = useUrlState<string | null>(
    "device_id",
    "string",
    null,
  );
  const [selectedUser, setSelectedUser] = useUrlState<string | null>(
    "user_id",
    "string",
    null,
  );
  const [galleryFullscreen, setGalleryFullscreen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [dayOffset, setDayOffset] = useUrlState("day", "number", 0);
  type RiskFilter = "all" | RiskRating;
  const [riskFilter, setRiskFilter] = useUrlState<RiskFilter>(
    "risk",
    "string",
    "all",
  );
  const [rawTypeFilter, setTypeFilter] = useUrlState<string | string[] | null>(
    "type",
    "string",
    null,
  );

  const { startMs: weekStart, endMs: weekEnd } = useMemo(
    () => getDayBounds(dayOffset),
    [dayOffset],
  );

  const {
    logs,
    batchStats,
    error: logsError,
    isLoading: logsLoading,
  } = useLogs({
    userId: selectedUser,
    deviceId: selectedDevice,
    startTime: weekStart,
    endTime: weekEnd,
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
  }

  const dayLabel = formatDayLabel(weekStart);
  const prevDay = useCallback(
    () => setDayOffset(dayOffset - 1),
    [dayOffset, setDayOffset],
  );
  const nextDay = useCallback(
    () => setDayOffset(dayOffset + 1),
    [dayOffset, setDayOffset],
  );

  const title =
    sidebarLoading && selectedUser
      ? "Loading…"
      : selectedUser
        ? `${groupLabel(selectedUser)}'s logs`
        : "My logs";
  const isGallery = path === "/logs/gallery";
  const typeFilter = Array.isArray(rawTypeFilter)
    ? (rawTypeFilter[0] ?? null)
    : rawTypeFilter;

  const items = useMemo(
    () =>
      (logs ?? []).filter((item) => {
        if (item.ts < weekStart || item.ts > weekEnd) return false;
        if (typeFilter !== null && typeFilter !== item.type) return false;
        if (riskFilter !== "all") {
          const rating = getRiskRating(item.risk);
          if (riskFilter === "high" && rating !== "high") return false;
          if (
            riskFilter === "moderate" &&
            rating !== "moderate" &&
            rating !== "high"
          )
            return false;
        }
        return true;
      }),
    [logs, riskFilter, typeFilter, weekStart, weekEnd],
  );
  const galleryItems = useMemo(
    () => items.filter((item) => getLogImage(item) !== undefined),
    [items],
  );

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
              <Button
                variant="ghost"
                size="md"
                class="logs-sidebar-toggle"
                type="button"
                onClick={() => setSidebarOpen(true)}
              >
                <MenuIcon />
                <span>Devices</span>
              </Button>
              <div class="logs-filter-switcher">
                <Field label="Risk" class="logs-filter-field">
                  <Select
                    size="md"
                    class="logs-filter-select"
                    value={riskFilter}
                    onChange={(e) =>
                      setRiskFilter(
                        (e.target as HTMLSelectElement).value as RiskFilter,
                      )
                    }
                  >
                    <option value="all">All</option>
                    <option value="high">High</option>
                    <option value="moderate">Medium</option>
                  </Select>
                </Field>
                <Field label="Type" class="logs-filter-field">
                  <Select
                    size="md"
                    class="logs-filter-select"
                    value={typeFilter ?? ""}
                    onChange={(e) =>
                      setTypeFilter(
                        (e.target as HTMLSelectElement).value || null,
                      )
                    }
                  >
                    <option value="">All</option>
                    {LOG_TYPES.map((type) => (
                      <option key={type} value={type}>
                        {humanizeLogType(type)}
                      </option>
                    ))}
                  </Select>
                </Field>
              </div>
              <div class="logs-header-view-controls">
                {isGallery && (
                  <IconButton
                    class="logs-fullscreen-btn"
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
                  </IconButton>
                )}
                <div class="vi-segmented-control logs-view-switcher">
                  <a
                    class={`vi-segmented-control__item${!isGallery ? " is-active" : ""}`}
                    href={`/logs${window.location.search}`}
                  >
                    List
                  </a>
                  <a
                    class={`vi-segmented-control__item${isGallery ? " is-active" : ""}`}
                    href={`/logs/gallery${window.location.search}`}
                  >
                    Gallery
                  </a>
                </div>
              </div>
            </div>
          </div>

          <div class="logs-week-nav">
            <IconButton aria-label="Previous day" onClick={prevDay}>
              <ChevronLeftIcon />
            </IconButton>
            <span class="logs-week-label">{dayLabel}</span>
            <IconButton
              aria-label="Next day"
              onClick={nextDay}
              disabled={dayOffset >= 0}
            >
              <ChevronRightIcon />
            </IconButton>
          </div>

          {logsError && <Alert variant="error">{logsError.message}</Alert>}
          {batchStats && batchStats.total > 0 && (
            <p class="logs-summary">
              {batchStats.decrypted}/{batchStats.total} block
              {batchStats.total === 1 ? "" : "s"} decrypted
              {batchStats.skipped > 0 && `, ${batchStats.skipped} unavailable`}
            </p>
          )}

          {isGallery ? (
            <LogsGallery
              items={galleryItems}
              loading={logsLoading}
              hasMore={false}
              onLoadMore={() => {}}
              deviceName={deviceName}
              fullscreen={galleryFullscreen}
            />
          ) : (
            <LogsList
              items={items}
              loading={logsLoading}
              hasMore={false}
              onLoadMore={() => {}}
              deviceName={deviceName}
            />
          )}
        </section>
      </div>
    </div>
  );
}
