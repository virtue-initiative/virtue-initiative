import { LogItem, LogImage } from './shared';

export function LogsList({
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
      <div class="log-list">
        {items.map((item) => (
          <div class="log-row" key={item.id}>
            <div class="log-thumb-wrap">
              <LogImage imageBytes={item.image} />
            </div>
            <div class="log-row-main">
              <div class="log-row-top">
                <span class="log-device">{deviceName(item.device_id)}</span>
                <span class="log-time">
                  {new Date(item.taken_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}
                </span>
              </div>
              <p class="log-device" style="margin:0.15rem 0 0;">
                {new Date(item.taken_at).toLocaleDateString()}
              </p>
            </div>
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
