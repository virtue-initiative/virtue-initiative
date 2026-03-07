import { groupLogsByDay, ImageLogItem, LogImage } from "./shared";
import { formatTime } from "../../utils/time";

export function LogsGallery({
  items,
  loading,
  hasMore,
  onLoadMore,
  deviceName,
}: {
  items: ImageLogItem[];
  loading: boolean;
  hasMore: boolean;
  onLoadMore: () => void;
  deviceName: (id: string) => string;
}) {
  if (items.length === 0 && !loading) {
    return <p class="empty">No screenshots found.</p>;
  }
  const dayGroups = groupLogsByDay(items);

  return (
    <>
      <div class="section-stack">
        {dayGroups.map((group) => (
          <section class="logs-day-group gallery-day-group" key={group.key}>
            <h2 class="section-heading">{group.label}</h2>
            <div class="gallery-grid">
              {group.items.map((item) => (
                <div
                  class={`gallery-item${item.batch_status === "failed" ? " gallery-item--unverified" : ""}`}
                  key={item.id}
                  title={`${deviceName(item.device_id)} — ${formatTime(item.taken_at)}${item.batch_status === "failed" ? " ⚠ Unverified" : ""}`}
                >
                  <LogImage imageBytes={item.image} />
                </div>
              ))}
            </div>
          </section>
        ))}
      </div>
      {loading && <p class="logs-loading">Loading…</p>}
      {!loading && hasMore && (
        <button
          class="btn btn-primary btn-sm load-more"
          onClick={onLoadMore}
          type="button"
        >
          Load more
        </button>
      )}
    </>
  );
}
