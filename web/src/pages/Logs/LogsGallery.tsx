import { LogItem, LogImage } from './shared';

export function LogsGallery({
  items,
  loading,
  hasMore,
  onLoadMore,
  deviceName,
}: {
  items: LogItem[];
  loading: boolean;
  hasMore: boolean;
  onLoadMore: () => void;
  deviceName: (id: string) => string;
}) {
  if (items.length === 0 && !loading) {
    return <p class="empty">No logs found.</p>;
  }

  return (
    <>
      <div class="gallery-grid">
        {items.map((item) => (
          <div
            class="gallery-item"
            key={item.id}
            title={`${deviceName(item.device_id)} — ${new Date(item.taken_at).toLocaleTimeString()}`}
          >
            <LogImage imageBytes={item.image} />
          </div>
        ))}
      </div>
      {loading && <p class="logs-loading">Loading…</p>}
      {!loading && hasMore && (
        <button class="btn btn-primary btn-sm load-more" onClick={onLoadMore} type="button">
          Load more
        </button>
      )}
    </>
  );
}
