import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { useLocation } from 'preact-iso';
import { Device, LogQueryResult, useAPIContext, useDevices, usePartners } from '../../utils/api';
import { LogsGallery } from './LogsGallery';
import { LogsList } from './LogsList';
import { getRiskRating, type RiskRating } from '@virtueinitiative/shared-web/risk';
import { FeedLog, formatDayLabel, getLogCategory, LOG_TYPES } from './shared';

const LOG_CATEGORIES = [
  ...new Set(
    LOG_TYPES.map((type) =>
      getLogCategory({ type, data: {}, id: '', device_id: '', ts: 0, created_at: 0 }),
    ),
  ),
];
import './style.css';
import { useUrlState } from '../../hooks/useUrlState';
import {
  Button,
  Dialog,
  DialogHeader,
  Field,
  IconButton,
  Select,
} from '@virtueinitiative/shared-web';
import { cacheClient } from '../../utils/cache/client';
import { formatRelativeTimestamp } from '../../utils/time';

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
      <path strokeLinecap="round" strokeLinejoin="round" d="M6 18 18 6M6 6l12 12" />
    </svg>
  );
}

function dateToBoundsStart(d: string): number {
  return new Date(d + 'T00:00:00').getTime();
}

function dateToBoundsEnd(d: string): number {
  return new Date(d + 'T23:59:59.999').getTime();
}

function shiftDate(dateStr: string, days: number): string {
  const d = new Date(dateStr + 'T00:00:00');
  d.setDate(d.getDate() + days);
  return d.toISOString().slice(0, 10);
}

export function Logs() {
  const api = useAPIContext();
  const userId = api?.userId ?? null;
  const { path } = useLocation();
  const { devices, loaded: devicesLoaded } = useDevices();
  const { watchings: watching, loaded: partnersLoaded } = usePartners();

  const today = new Date().toISOString().slice(0, 10);
  const yesterday = shiftDate(today, -1);
  const oneMonthAgo = (() => {
    const d = new Date();
    d.setMonth(d.getMonth() - 1);
    return d.toISOString().slice(0, 10);
  })();

  const [selectedDevice, setSelectedDevice] = useUrlState<string | null>(
    'device_id',
    'string',
    null,
  );
  const [selectedUser, setSelectedUser] = useUrlState<string | null>('user_id', 'string', null);
  const [galleryFullscreen, setGalleryFullscreen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [startDate, setStartDate] = useUrlState('start', 'string', yesterday);
  const [endDate, setEndDate] = useUrlState('end', 'string', today);
  const [visibleDate, setVisibleDate] = useState<string | null>(null);
  const filterDialogRef = useRef<HTMLDialogElement>(null);
  type RiskFilter = 'all' | RiskRating;
  const [riskFilter, setRiskFilter] = useUrlState<RiskFilter>('risk', 'string', 'all');
  const [rawTypeFilter, setTypeFilter] = useUrlState<string | string[] | null>(
    'type',
    'string',
    null,
  );

  const weekStart = dateToBoundsStart(startDate);
  const weekEnd = dateToBoundsEnd(endDate);

  const [logResult, setLogResult] = useState<LogQueryResult>({
    logs: [],
    complete: false,
    processed: 0,
    total: 0,
  });
  const activeTargetUserId = selectedUser ?? userId;
  const scopeKeyRef = useRef<string | null>(null);

  useEffect(() => {
    if (!api || !activeTargetUserId) {
      setLogResult({ logs: [], complete: false, processed: 0, total: 0 });
      scopeKeyRef.current = null;
      return;
    }

    const newScopeKey = `${activeTargetUserId}:${selectedDevice}`;
    const scopeChanged = scopeKeyRef.current !== newScopeKey;
    scopeKeyRef.current = newScopeKey;

    let cancelled = false;
    const initial = api.queryLogs(
      {
        userId: activeTargetUserId,
        deviceId: selectedDevice ?? undefined,
        startTime: weekStart,
        endTime: weekEnd,
      },
      (next) => {
        if (!cancelled) setLogResult(next);
      },
    );
    if (scopeChanged) {
      setLogResult(initial);
    }
    return () => {
      cancelled = true;
    };
  }, [api, activeTargetUserId, selectedDevice, weekStart, weekEnd]);

  const logs: FeedLog[] = logResult.logs;
  const logsLoading = !logResult.complete;
  const deviceList = devices;
  const watchingList = watching;
  const sidebarLoading = !devicesLoaded || !partnersLoaded;

  const selectedDeviceInfo = selectedDevice
    ? (deviceList.find((d) => d.id === selectedDevice) ?? null)
    : null;

  const [estimatedNextUpload, setEstimatedNextUpload] = useState<number | null>(null);

  useEffect(() => {
    if (!userId || !activeTargetUserId || !selectedDevice || !selectedDeviceInfo?.last_upload_at) {
      setEstimatedNextUpload(null);
      return;
    }
    cacheClient
      ?.getDeviceBatchEndTimes(userId, activeTargetUserId, selectedDevice)
      .then((endTimes) => {
        if (endTimes.length < 2) {
          setEstimatedNextUpload(null);
          return;
        }
        const intervals = endTimes.slice(1).map((t, i) => t - endTimes[i]);
        intervals.sort((a, b) => a - b);
        const median = intervals[Math.floor(intervals.length / 2)];
        if (median > 0 && selectedDeviceInfo.last_upload_at) {
          setEstimatedNextUpload(selectedDeviceInfo.last_upload_at + median);
        }
      })
      .catch(() => {});
  }, [userId, activeTargetUserId, selectedDevice, selectedDeviceInfo?.last_upload_at]);

  const { knownUsers, deviceGroups } = useMemo(() => {
    if (!userId) {
      return {
        knownUsers: [] as UserLabel[],
        deviceGroups: [] as DeviceGroup[],
      };
    }

    const labels = new Map<string, string>();
    labels.set(userId, 'My devices');
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
        .sort(([a], [b]) => (a === userId ? -1 : b === userId ? 1 : a.localeCompare(b)))
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
    if (sidebarLoading) {
      return;
    }

    if (selectedUser !== null && !deviceGroups.some((group) => group.userId === selectedUser)) {
      select(null, null);
      return;
    }

    if (selectedDevice && !activeDeviceIds.has(selectedDevice)) {
      select(selectedUser, null);
    }
  }, [activeDeviceIds, deviceGroups, selectedDevice, selectedUser, sidebarLoading]);

  const allDevices = useMemo(() => deviceGroups.flatMap((group) => group.devices), [deviceGroups]);

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

  const baseTitle =
    sidebarLoading && selectedUser
      ? 'Loading…'
      : selectedUser
        ? `${groupLabel(selectedUser)}'s logs`
        : 'My logs';
  const title = selectedDevice ? `${baseTitle} — ${deviceName(selectedDevice)}` : baseTitle;
  const isGallery = path === '/logs/gallery';
  const typeFilter = Array.isArray(rawTypeFilter) ? (rawTypeFilter[0] ?? null) : rawTypeFilter;

  const items = useMemo(
    () =>
      (logs ?? []).filter((item) => {
        if (item.ts < weekStart || item.ts > weekEnd) return false;
        if (typeFilter !== null && getLogCategory({ ...item, data: {} }) !== typeFilter)
          return false;
        if (riskFilter !== 'all') {
          const rating = getRiskRating(item.risk);
          if (riskFilter === 'high' && rating !== 'high') return false;
          if (riskFilter === 'moderate' && rating !== 'moderate' && rating !== 'high') return false;
        }
        return true;
      }),
    [logs, riskFilter, typeFilter, weekStart, weekEnd],
  );
  const galleryItems = useMemo(
    () =>
      (logs ?? []).filter((item) => {
        if (item.ts < weekStart || item.ts > weekEnd) return false;
        if (item.image_w === undefined) return false;
        if (riskFilter !== 'all') {
          const rating = getRiskRating(item.risk);
          if (riskFilter === 'high' && rating !== 'high') return false;
          if (riskFilter === 'moderate' && rating !== 'moderate' && rating !== 'high') return false;
        }
        return true;
      }),
    [logs, riskFilter, weekStart, weekEnd],
  );

  useEffect(() => {
    if (!isGallery) {
      setGalleryFullscreen(false);
    }
  }, [isGallery]);

  return (
    <div
      class={`logs-page${isGallery && galleryFullscreen ? ' logs-page--gallery-fullscreen' : ''}`}
    >
      <button
        class={`app-drawer-backdrop logs-sidebar-backdrop${sidebarOpen ? ' is-open' : ''}`}
        type="button"
        aria-label="Close logs sidebar"
        onClick={() => setSidebarOpen(false)}
      />
      <div class="logs-layout">
        {!(isGallery && galleryFullscreen) && (
          <aside class={`logs-sidebar${sidebarOpen ? ' is-open' : ''}`}>
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
            {sidebarLoading && <p class="logs-sidebar-loading">Loading…</p>}
            {!sidebarLoading && deviceGroups.length === 0 && (
              <div class="logs-sidebar-group">
                <p class="logs-sidebar-group-label">My devices</p>
                <p class="logs-sidebar-loading">No devices</p>
              </div>
            )}
            {deviceGroups.map((group) => (
              <div class="logs-sidebar-group" key={group.label}>
                <button
                  class={`logs-device-button logs-device-button-group${selectedUser === group.userId && selectedDevice === null ? ' is-active' : ''}`}
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
                        class={`logs-device-button${selectedDevice === device.id ? ' is-active' : ''}`}
                        onClick={() => select(group.userId, device.id)}
                        type="button"
                        title={device.name}
                      >
                        <span
                          class={`logs-status-dot ${device.status === 'online' ? 'logs-status-dot--online' : 'logs-status-dot--offline'}`}
                        />
                        <span class="logs-device-button-label">{device.name}</span>
                      </button>
                    </li>
                  ))}
                </ul>
                {group.devices.length === 0 && <p class="logs-sidebar-loading">No devices</p>}
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
              <div class="logs-filter-section">
                <div class="logs-inline-filters">
                  <Field label="Start" class="logs-filter-field">
                    <input
                      type="date"
                      class="logs-filter-date"
                      value={startDate}
                      min={oneMonthAgo}
                      max={endDate}
                      onChange={(e) => setStartDate((e.target as HTMLInputElement).value)}
                    />
                  </Field>
                  <Field label="End" class="logs-filter-field">
                    <input
                      type="date"
                      class="logs-filter-date"
                      value={endDate}
                      min={oneMonthAgo}
                      max={today}
                      onChange={(e) => setEndDate((e.target as HTMLInputElement).value)}
                    />
                  </Field>
                  <Field label="Risk" class="logs-filter-field">
                    <Select
                      size="md"
                      class="logs-filter-select"
                      value={riskFilter}
                      onChange={(e) =>
                        setRiskFilter((e.target as HTMLSelectElement).value as RiskFilter)
                      }
                    >
                      <option value="all">All</option>
                      <option value="high">High</option>
                      <option value="moderate">Medium</option>
                    </Select>
                  </Field>
                  {!isGallery && (
                    <Field label="Type" class="logs-filter-field">
                      <Select
                        size="md"
                        class="logs-filter-select"
                        value={typeFilter ?? ''}
                        onChange={(e) =>
                          setTypeFilter((e.target as HTMLSelectElement).value || null)
                        }
                      >
                        <option value="">All</option>
                        {LOG_CATEGORIES.map((cat) => (
                          <option key={cat} value={cat}>
                            {cat}
                          </option>
                        ))}
                      </Select>
                    </Field>
                  )}
                </div>
                <Button
                  variant="ghost"
                  size="md"
                  class="logs-filter-toggle"
                  type="button"
                  onClick={() => filterDialogRef.current?.showModal()}
                >
                  Edit Search
                </Button>
              </div>
              <div class="logs-header-view-controls">
                {isGallery && (
                  <IconButton
                    class="logs-fullscreen-btn"
                    onClick={() => setGalleryFullscreen((prev) => !prev)}
                    aria-label={galleryFullscreen ? 'Exit fullscreen' : 'Fullscreen'}
                    title={galleryFullscreen ? 'Exit fullscreen' : 'Fullscreen'}
                  >
                    {galleryFullscreen ? <ExitFullscreenIcon /> : <ExpandIcon />}
                  </IconButton>
                )}
                <div class="vi-segmented-control logs-view-switcher">
                  <a
                    class={`vi-segmented-control__item${!isGallery ? ' is-active' : ''}`}
                    href={`/logs${window.location.search}`}
                  >
                    List
                  </a>
                  <a
                    class={`vi-segmented-control__item${isGallery ? ' is-active' : ''}`}
                    href={`/logs/gallery${window.location.search}`}
                  >
                    Gallery
                  </a>
                </div>
              </div>
            </div>
          </div>

          <p class="logs-summary">
            {logsLoading
              ? logResult.total > 0
                ? `Syncing logs… ${logResult.processed}/${logResult.total} blocks`
                : 'Syncing logs…'
              : 'Logs synced'}
            {!logsLoading && selectedDeviceInfo && selectedDeviceInfo.pending_count > 0 && (
              <>
                {` · ${selectedDeviceInfo.pending_count} item${selectedDeviceInfo.pending_count !== 1 ? 's' : ''} pending upload`}
                {estimatedNextUpload && estimatedNextUpload > Date.now()
                  ? ` · expected ${formatRelativeTimestamp(estimatedNextUpload)}`
                  : null}
              </>
            )}
          </p>

          <div class="logs-sticky-date" aria-live="polite">
            {visibleDate ?? formatDayLabel(weekStart)}
          </div>

          {isGallery ? (
            <LogsGallery
              items={galleryItems}
              loading={logsLoading}
              hasMore={false}
              onLoadMore={() => {}}
              deviceName={deviceName}
              fullscreen={galleryFullscreen}
              onVisibleDateChange={setVisibleDate}
              viewerId={userId ?? ''}
            />
          ) : (
            <LogsList
              items={items}
              loading={logsLoading}
              hasMore={false}
              onLoadMore={() => {}}
              deviceName={deviceName}
              onVisibleDateChange={setVisibleDate}
              viewerId={userId ?? ''}
            />
          )}

          <div class="logs-load-more">
            <Button
              variant="ghost"
              size="md"
              type="button"
              onClick={() =>
                setStartDate(
                  shiftDate(startDate, -1) >= oneMonthAgo ? shiftDate(startDate, -1) : oneMonthAgo,
                )
              }
            >
              Load another day
            </Button>
            <Button
              variant="ghost"
              size="md"
              type="button"
              onClick={() =>
                setStartDate(
                  shiftDate(startDate, -7) >= oneMonthAgo ? shiftDate(startDate, -7) : oneMonthAgo,
                )
              }
            >
              Load another week
            </Button>
          </div>

          <Dialog dialogRef={filterDialogRef} size="lg" class="logs-filter-dialog">
            <DialogHeader>Search filters</DialogHeader>
            <div class="logs-filter-dialog-fields">
              <Field label="Start" class="logs-filter-field">
                <input
                  type="date"
                  class="logs-filter-date"
                  value={startDate}
                  min={oneMonthAgo}
                  max={endDate}
                  onChange={(e) => setStartDate((e.target as HTMLInputElement).value)}
                />
              </Field>
              <Field label="End" class="logs-filter-field">
                <input
                  type="date"
                  class="logs-filter-date"
                  value={endDate}
                  min={oneMonthAgo}
                  max={today}
                  onChange={(e) => setEndDate((e.target as HTMLInputElement).value)}
                />
              </Field>
              <Field label="Risk" class="logs-filter-field">
                <Select
                  size="md"
                  class="logs-filter-select"
                  value={riskFilter}
                  onChange={(e) =>
                    setRiskFilter((e.target as HTMLSelectElement).value as RiskFilter)
                  }
                >
                  <option value="all">All</option>
                  <option value="high">High</option>
                  <option value="moderate">Medium</option>
                </Select>
              </Field>
              {!isGallery && (
                <Field label="Type" class="logs-filter-field">
                  <Select
                    size="md"
                    class="logs-filter-select"
                    value={typeFilter ?? ''}
                    onChange={(e) => setTypeFilter((e.target as HTMLSelectElement).value || null)}
                  >
                    <option value="">All</option>
                    {LOG_CATEGORIES.map((cat) => (
                      <option key={cat} value={cat}>
                        {cat}
                      </option>
                    ))}
                  </Select>
                </Field>
              )}
            </div>
          </Dialog>
        </section>
      </div>
    </div>
  );
}
