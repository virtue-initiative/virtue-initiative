import { useState, useEffect } from 'preact/hooks';
import { Log } from '../../api';
import { LogImage, relativeTime } from './shared';

const PAGE_SIZE = 100;

export function LogsGallery({
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
      const withImages = page.items.filter((l) => l.image_url);
      setLogs((prev) => (reset ? withImages : [...prev, ...withImages]));
      setNextCursor(page.next_cursor);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load images');
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      {error && <p class="error-banner">{error}</p>}
      {logs.length === 0 && !loading ? (
        <p class="empty">No images found.</p>
      ) : (
        <div class="gallery-grid">
          {logs.map((log) => (
            <GalleryItem key={log.id} log={log} token={token} deviceName={deviceName(log.device_id)} />
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

function GalleryItem({ log, token, deviceName }: { log: Log; token: string; deviceName: string }) {
  return (
    <div class="gallery-item" title={`${deviceName} · ${relativeTime(log.created_at)}`}>
      <LogImage token={token} imageUrl={log.image_url!} />
    </div>
  );
}
