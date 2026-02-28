import { useState, useEffect } from 'preact/hooks';
import { useLocation } from 'preact-iso';
import { useAuth } from '../../context/auth';
import { useE2EE } from '../../context/e2ee';
import { api, Batch, Device } from '../../api';
import { decryptBatch, decompressGzip } from '../../crypto';
import { decode } from '@msgpack/msgpack';
import { LogItem } from './shared';
import { LogsList } from './LogsList';
import { LogsGallery } from './LogsGallery';
import './style.css';

interface DeviceGroup {
  label: string;
  userId: string | null;
  devices: Device[];
}

interface RawBlobItem {
  id: string;
  taken_at: number;
  kind: string;
  image?: Uint8Array | number[];
  metadata: [string, string][];
}

function toUint8Array(val: Uint8Array | number[] | undefined): Uint8Array | undefined {
  if (!val) return undefined;
  if (val instanceof Uint8Array) return val;
  return new Uint8Array(val);
}

async function decryptAndFlattenBatch(batch: Batch, key: CryptoKey): Promise<LogItem[]> {
  const r2Base = (import.meta as any).env?.VITE_R2_URL ?? '';
  const url = `${r2Base}/${batch.r2_key}`;
  const resp = await fetch(url);
  if (!resp.ok) throw new Error(`Fetch failed (${resp.status}) for ${url}`);
  const raw = new Uint8Array(await resp.arrayBuffer());
  if (raw.length < 13) throw new Error(`Batch blob too short for AES-GCM payload: ${url}`);
  const decrypted = await decryptBatch(key, raw);
  const decompressed = await decompressGzip(decrypted);
  const decoded = decode(decompressed);
  if (!decoded || typeof decoded !== 'object' || !('items' in (decoded as object))) {
    console.error(`[batch ${batch.id}] unexpected decoded structure:`, decoded);
    return [];
  }
  const blob = decoded as { version: number; items: RawBlobItem[] };
  const items = blob.items ?? [];
  const screenshots = items.filter((item) => item.kind === 'screenshot' && item.image);
  const skipped = items.length - screenshots.length;
  return screenshots.map((item) => ({
    id: item.id,
    taken_at: item.taken_at,
    device_id: batch.device_id,
    image: toUint8Array(item.image)!,
  }));
}

export function Logs() {
  const { token } = useAuth();
  const e2ee = useE2EE();
  const { path } = useLocation();

  const [deviceGroups, setDeviceGroups] = useState<DeviceGroup[] | null>(null);
  const [selectedDevice, setSelectedDevice] = useState<string | null>(
    () => new URLSearchParams(window.location.search).get('device_id'),
  );
  const [selectedUser, setSelectedUser] = useState<string | null>(
    () => new URLSearchParams(window.location.search).get('user'),
  );
  const [loadError, setLoadError] = useState<string | null>(null);

  const [items, setItems] = useState<LogItem[]>([]);
  const [nextCursor, setNextCursor] = useState<string | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [fetchError, setFetchError] = useState<string | null>(null);

  const [password, setPassword] = useState('');
  const [pwError, setPwError] = useState<string | null>(null);
  const [pwLoading, setPwLoading] = useState(false);

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

  // Load batches when key or filter changes
  useEffect(() => {
    if (!e2ee.key || !token) return;
    setItems([]);
    setNextCursor(undefined);
    doLoadBatches(undefined, true);
  }, [e2ee.key, token, selectedDevice, selectedUser]);

  async function doLoadBatches(cursor: string | undefined, reset: boolean) {
    if (!e2ee.key || !token) return;
    setLoading(true);
    setFetchError(null);
    try {
      const page = await api.getBatches(token, {
        user: selectedUser ?? undefined,
        device_id: selectedDevice ?? undefined,
        cursor,
        limit: 10,
      });
      const nested = await Promise.allSettled(page.items.map((b) => decryptAndFlattenBatch(b, e2ee.key!)));
      const flat: LogItem[] = [];
      for (const result of nested) {
        if (result.status === 'fulfilled') {
          flat.push(...result.value);
        } else {
          console.error('[logs] failed to decrypt batch:', result.reason);
        }
      }
      setItems((prev) => {
        const combined = reset ? flat : [...prev, ...flat];
        return combined.sort((a, b) => b.taken_at - a.taken_at);
      });
      setNextCursor(page.next_cursor);
    } catch (err) {
      console.error('[logs] load failed:', err);
      setFetchError(err instanceof Error ? err.message : 'Failed to load logs');
    } finally {
      setLoading(false);
    }
  }

  async function handlePasswordSubmit(e: Event) {
    e.preventDefault();
    setPwError(null);
    setPwLoading(true);
    try {
      // JWT uses base64url (no padding, uses - and _); atob needs standard base64
      const b64 = token!.split('.')[1].replace(/-/g, '+').replace(/_/g, '/');
      const padded = b64 + '='.repeat((4 - (b64.length % 4)) % 4);
      const payload = JSON.parse(atob(padded));
      if (!payload.sub) throw new Error('Could not determine user ID from token');
      await e2ee.setKey(password, payload.sub);
    } catch (err) {
      setPwError(err instanceof Error ? err.message : 'Failed to derive key');
    } finally {
      setPwLoading(false);
    }
  }

  const allDevices = deviceGroups?.flatMap((g) => g.devices) ?? [];
  const deviceName = (id: string) => allDevices.find((d) => d.id === id)?.name ?? id.slice(0, 8) + '…';
  const groupLabel = (userId: string) =>
    deviceGroups?.find((g) => g.userId === userId)?.label ?? userId.slice(0, 8) + '…';

  function select(userId: string | null, deviceId: string | null) {
    setSelectedUser(userId);
    setSelectedDevice(deviceId);
    const qs = new URLSearchParams(window.location.search);
    if (deviceId) qs.set('device_id', deviceId); else qs.delete('device_id');
    if (userId) qs.set('user', userId); else qs.delete('user');
    const search = qs.toString();
    const newUrl = window.location.pathname + (search ? `?${search}` : '');
    window.history.replaceState(null, '', newUrl);
  }

  const title = selectedDevice
    ? `${selectedUser ? groupLabel(selectedUser) + ' — ' : ''}${deviceName(selectedDevice)}`
    : selectedUser
    ? `${groupLabel(selectedUser)}'s logs`
    : 'All logs';

  const isGallery = path === '/logs/gallery';

  return (
    <div class="logs-layout">
      {!e2ee.key && (
        <div class="pw-overlay">
          <div class="pw-card">
            <h2 class="pw-title">Decrypt logs</h2>
            <p class="pw-desc">Enter your E2EE password to view logs.</p>
            <form class="pw-form" onSubmit={handlePasswordSubmit}>
              <input
                type="password"
                value={password}
                onInput={(e) => setPassword((e.target as HTMLInputElement).value)}
                placeholder="Decryption password"
                required
                autoFocus
              />
              <button class="btn btn-primary" type="submit" disabled={pwLoading}>
                {pwLoading ? 'Unlocking…' : 'Unlock'}
              </button>
              {pwError && <p class="form-error">{pwError}</p>}
            </form>
          </div>
        </div>
      )}

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
            <a class={`view-tab${!isGallery ? ' active' : ''}`} href="/logs">
              List
            </a>
            <a class={`view-tab${isGallery ? ' active' : ''}`} href="/logs/gallery">
              Gallery
            </a>
          </div>
        </div>

        {fetchError && <p class="error-banner">{fetchError}</p>}

        {isGallery ? (
          <LogsGallery
            items={items}
            loading={loading}
            hasMore={!!nextCursor}
            onLoadMore={() => doLoadBatches(nextCursor, false)}
            deviceName={deviceName}
          />
        ) : (
          <LogsList
            items={items}
            loading={loading}
            hasMore={!!nextCursor}
            onLoadMore={() => doLoadBatches(nextCursor, false)}
            deviceName={deviceName}
          />
        )}
      </section>
    </div>
  );
}
