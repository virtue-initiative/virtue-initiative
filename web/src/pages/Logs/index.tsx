import { useState, useEffect } from 'preact/hooks';
import { useRoute } from 'preact-iso';
import { useAuth } from '../../context/auth';
import { api, Device } from '../../api';
import { LogsList } from './LogsList';
import { LogsGallery } from './LogsGallery';
import './style.css';

interface DeviceGroup {
  label: string;
  userId: string | null;
  devices: Device[];
}

type View = 'list' | 'gallery';

export function Logs() {
  const { token } = useAuth();
  const { path } = useRoute();
  const view: View = path === '/logs/gallery' ? 'gallery' : 'list';

  const [deviceGroups, setDeviceGroups] = useState<DeviceGroup[] | null>(null);
  const [selectedDevice, setSelectedDevice] = useState<string | null>(
    () => new URLSearchParams(window.location.search).get('device_id'),
  );
  const [selectedUser, setSelectedUser] = useState<string | null>(
    () => new URLSearchParams(window.location.search).get('user'),
  );
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    if (!token) return;
    Promise.all([api.getDevices(token), api.getPartners(token)])
      .then(async ([myDevices, partners]) => {
        const monitored = partners.filter((p) => p.role === 'partner' && p.status === 'accepted');
        const partnerGroups = await Promise.all(
          monitored.map(async (p) => {
            const devs = await api.getDevices(token, { user: p.partner_user_id }).catch(() => [] as Device[]);
            return { label: p.partner_email, userId: p.partner_user_id, devices: devs };
          }),
        );
        setDeviceGroups([
          { label: 'My devices', userId: null, devices: myDevices },
          ...partnerGroups,
        ]);
      })
      .catch((err) => setLoadError(err instanceof Error ? err.message : 'Failed to load devices'));
  }, [token]);

  const allDevices = deviceGroups?.flatMap((g) => g.devices) ?? [];
  const deviceName = (id: string) => allDevices.find((d) => d.id === id)?.name ?? id.slice(0, 8) + '…';
  const groupLabel = (userId: string) =>
    deviceGroups?.find((g) => g.userId === userId)?.label ?? userId.slice(0, 8) + '…';

  function select(userId: string | null, deviceId: string | null) {
    setSelectedUser(userId);
    setSelectedDevice(deviceId);
  }

  const title = selectedDevice
    ? `${selectedUser ? groupLabel(selectedUser) + ' — ' : ''}${deviceName(selectedDevice)}`
    : selectedUser
    ? `${groupLabel(selectedUser)}'s logs`
    : 'All logs';

  function viewHref(v: View) {
    const qs = window.location.search;
    return v === 'gallery' ? `/logs/gallery${qs}` : `/logs${qs}`;
  }

  return (
    <div class="logs-layout">
      <aside class="logs-sidebar">
        {loadError && <p class="sidebar-loading">{loadError}</p>}
        {deviceGroups === null && !loadError && <p class="sidebar-loading">Loading…</p>}
        {deviceGroups?.map((group) => (
          <div class="sidebar-group" key={group.label}>
            <p class="sidebar-group-label">{group.label}</p>
            <ul class="device-list">
              <li>
                <button
                  class={`device-btn${selectedUser === group.userId && selectedDevice === null ? ' active' : ''}`}
                  onClick={() => select(group.userId, null)}
                  type="button"
                >
                  All
                </button>
              </li>
              {group.devices.map((d) => (
                <li key={d.id}>
                  <button
                    class={`device-btn${selectedDevice === d.id ? ' active' : ''}`}
                    onClick={() => select(group.userId, d.id)}
                    type="button"
                  >
                    <span class={`dot ${d.status === 'online' ? 'dot-green' : 'dot-gray'}`} />
                    {d.name}
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
            <a href={viewHref('list')} class={`view-tab${view === 'list' ? ' active' : ''}`}>List</a>
            <a href={viewHref('gallery')} class={`view-tab${view === 'gallery' ? ' active' : ''}`}>Gallery</a>
          </div>
        </div>

        {view === 'list' && (
          <LogsList
            token={token!}
            selectedUser={selectedUser}
            selectedDevice={selectedDevice}
            deviceName={deviceName}
          />
        )}
        {view === 'gallery' && (
          <LogsGallery
            token={token!}
            selectedUser={selectedUser}
            selectedDevice={selectedDevice}
            deviceName={deviceName}
          />
        )}
      </section>
    </div>
  );
}
