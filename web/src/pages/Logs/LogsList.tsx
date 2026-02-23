import { useState, useEffect } from 'preact/hooks';
import { Log } from '../../api';
import { LogImage, relativeTime } from './shared';

const PAGE_SIZE = 50;

export function LogsList({
  token,
  selectedUser,
  selectedDevice,
  deviceName,
}: {
  token: string;
  selectedUser: string | null;
  selectedDevice: string | null;
  deviceName: (id: string) => string;
}) {
  const [logs, setLogs] = useState<Log[]>([]);
  const [nextCursor, setNextCursor] = useState<string | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setLogs([]);
    setNextCursor(undefined);
    fetchLogs(true);
  }, [token, selectedDevice, selectedUser]);

  async function fetchLogs(reset = false) {
    const { api } = await import('../../api');
    setLoading(true);
    setError(null);
    try {
      const page = await api.getLogs(token, {
        user: selectedUser ?? undefined,
        device_id: selectedDevice ?? undefined,
        cursor: reset ? undefined : nextCursor,
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

  return (
    <>
      {error && <p class="error-banner">{error}</p>}
      {logs.length === 0 && !loading ? (
        <p class="empty">No logs found.</p>
      ) : (
        <div class="log-list">
          {logs.map((log) => (
            <LogRow
              key={log.id}
              log={log}
              token={token}
              deviceName={deviceName(log.device_id)}
              showDevice={selectedDevice === null}
            />
          ))}
        </div>
      )}
      {loading && <p class="logs-loading">Loading…</p>}
      {!loading && nextCursor && (
        <button class="btn btn-primary btn-sm load-more" onClick={() => fetchLogs(false)} type="button">
          Load more
        </button>
      )}
    </>
  );
}

function LogRow({
  log,
  token,
  deviceName,
  showDevice,
}: {
  log: Log;
  token: string;
  deviceName: string;
  showDevice: boolean;
}) {
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
      {log.image_url && <LogImage token={token} imageUrl={log.image_url} />}
    </div>
  );
}
