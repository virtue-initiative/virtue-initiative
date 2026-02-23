import { useState, useEffect } from 'preact/hooks';
import { useAuth } from '../../context/auth';
import { api, Device, Log } from '../../api';
import './style.css';

const PAGE_SIZE = 50;

export function Logs() {
  const { token } = useAuth();
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [selectedDevice, setSelectedDevice] = useState<string | null>(null);
  const [logs, setLogs] = useState<Log[]>([]);
  const [nextCursor, setNextCursor] = useState<string | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!token) return;
    api.getDevices(token)
      .then(setDevices)
      .catch((err) => setError(err instanceof Error ? err.message : 'Failed to load devices'));
  }, [token]);

  useEffect(() => {
    if (!token) return;
    setLogs([]);
    setNextCursor(undefined);
    fetchLogs(true);
  }, [token, selectedDevice]);

  async function fetchLogs(reset = false) {
    if (!token) return;
    setLoading(true);
    setError(null);
    try {
      const cursor = reset ? undefined : nextCursor;
      const page = await api.getLogs(token, {
        device_id: selectedDevice ?? undefined,
        cursor,
        limit: PAGE_SIZE,
      });
      setLogs((prev) => (reset ? page.items : [...prev, ...page.items]));
      setNextCursor(page.next_cursor);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load logs');
    } finally {
      setLoading(false);
    }
  }

  const deviceName = (id: string) =>
    devices?.find((d) => d.id === id)?.name ?? id.slice(0, 8) + '…';

  return (
    <div class="logs-layout">
      <aside class="logs-sidebar">
        <h2>Devices</h2>
        <ul class="device-list">
          <li>
            <button
              class={`device-btn${selectedDevice === null ? ' active' : ''}`}
              onClick={() => setSelectedDevice(null)}
              type="button"
            >
              All devices
            </button>
          </li>
          {devices === null ? (
            <li class="sidebar-loading">Loading…</li>
          ) : (
            devices.map((d) => (
              <li key={d.id}>
                <button
                  class={`device-btn${selectedDevice === d.id ? ' active' : ''}`}
                  onClick={() => setSelectedDevice(d.id)}
                  type="button"
                >
                  <span class={`dot ${d.status === 'online' ? 'dot-green' : 'dot-gray'}`} />
                  {d.name}
                </button>
              </li>
            ))
          )}
        </ul>
      </aside>

      <section class="logs-main">
        <div class="logs-header">
          <h1>
            {selectedDevice ? `Logs — ${deviceName(selectedDevice)}` : 'All logs'}
          </h1>
        </div>

        {error && <p class="error-banner">{error}</p>}

        {logs.length === 0 && !loading ? (
          <p class="empty">No logs found.</p>
        ) : (
          <div class="log-list">
            {logs.map((log) => (
              <LogRow key={log.id} log={log} deviceName={deviceName(log.device_id)} showDevice={selectedDevice === null} />
            ))}
          </div>
        )}

        {loading && <p class="logs-loading">Loading…</p>}

        {!loading && nextCursor && (
          <button class="btn btn-primary btn-sm load-more" onClick={() => fetchLogs(false)} type="button">
            Load more
          </button>
        )}
      </section>
    </div>
  );
}

function LogRow({
  log,
  deviceName,
  showDevice,
}: {
  log: Log;
  deviceName: string;
  showDevice: boolean;
}) {
  const [imgOpen, setImgOpen] = useState(false);

  return (
    <div class="log-row">
      <div class="log-row-main">
        <div class="log-row-top">
          <span class="log-type">{log.type}</span>
          {showDevice && <span class="log-device">{deviceName}</span>}
          <span class="log-time">{relativeTime(log.created_at)}</span>
        </div>
        {log.metadata && Object.keys(log.metadata).length > 0 && (
          <dl class="log-meta">
            {Object.entries(log.metadata).map(([k, v]) => (
              <>
                <dt key={`k-${k}`}>{k}</dt>
                <dd key={`v-${k}`}>{String(v)}</dd>
              </>
            ))}
          </dl>
        )}
      </div>
      {log.image_url && (
        <div class="log-thumb-wrap">
          <button class="log-thumb-btn" type="button" onClick={() => setImgOpen(true)} aria-label="View image">
            <img
              class="log-thumb"
              src={log.image_url}
              alt="log capture"
              loading="lazy"
            />
          </button>
          {imgOpen && (
            <div class="img-overlay" onClick={() => setImgOpen(false)}>
              <img class="img-full" src={log.image_url} alt="log capture" onClick={(e) => e.stopPropagation()} />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return 'Just now';
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}
