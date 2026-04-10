import { formatTime } from "../../utils/time";
import { FeedLog, getLogImage, groupLogsByDay, LogImage } from "./shared";

export function LogsGallery({
  items,
  loading,
  hasMore,
  onLoadMore,
  deviceName,
  fullscreen,
}: {
  items: FeedLog[];
  loading: boolean;
  hasMore: boolean;
  onLoadMore: () => void;
  deviceName: (id: string) => string;
  fullscreen: boolean;
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
            <div
              class={`gallery-grid${fullscreen ? " gallery-grid--fullscreen" : ""}`}
            >
              {group.items.map((item) => {
                const image = getLogImage(item);
                if (!image) {
                  return null;
                }

                return (
                  <div
                    class={`gallery-item${item.batch_status === "failed" ? " gallery-item--unverified" : ""}`}
                    key={item.id}
                    title={`${deviceName(item.device_id)} — ${formatTime(item.ts)}${item.batch_status === "failed" ? " ⚠ Unverified" : ""}`}
                  >
                    <LogImage imageBytes={image} />
                  </div>
                );
              })}
            </div>
          </section>
        ))}
      </div>
      {loading && <p class="logs-loading">Loading…</p>}
      {!loading && hasMore && (
        <button
          class="btn btn-primary load-more"
          onClick={onLoadMore}
          type="button"
        >
          Load more
        </button>
      )}
    </>
  );
}
