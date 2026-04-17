import { useEffect, useRef } from "preact/hooks";
import { formatDate, formatTime } from "../../utils/time";
import {
  describeRiskLevel,
  FeedLog,
  getLogImage,
  groupLogsByDay,
  LogImage,
} from "./shared";

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
  const loadSentinelRef = useRef<HTMLDivElement>(null);
  const loadRequestedRef = useRef(false);

  useEffect(() => {
    if (!loading) {
      loadRequestedRef.current = false;
    }
  }, [loading]);

  useEffect(() => {
    if (!hasMore || loading) {
      return;
    }

    const sentinel = loadSentinelRef.current;
    if (!sentinel) {
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        const isVisible = entries.some((entry) => entry.isIntersecting);
        if (!isVisible || loadRequestedRef.current) {
          return;
        }

        loadRequestedRef.current = true;
        onLoadMore();
      },
      { rootMargin: "280px 0px" },
    );

    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [hasMore, loading, onLoadMore]);

  return (
    <>
      <div class="section-stack">
        {dayGroups.map((group) => (
          <section
            class="logs-day-group logs-gallery-day-group"
            key={group.key}
          >
            <h2 class="section-heading">{group.label}</h2>
            <div
              class={`logs-gallery-grid${fullscreen ? " logs-gallery-grid--fullscreen" : ""}`}
            >
              {group.items.map((item) => {
                const image = getLogImage(item);
                if (!image) {
                  return null;
                }
                const riskLabel =
                  describeRiskLevel(item.risk) ?? "Risk unavailable";
                const previewTitle = `${formatDate(item.ts)} ${formatTime(item.ts)}`;
                const previewSubtitle = `${riskLabel}${item.batch_status === "failed" ? " • Unverified" : ""}`;

                return (
                  <div
                    class={`logs-gallery-item${item.batch_status === "failed" ? " logs-gallery-item--unverified" : ""}`}
                    key={item.id}
                    title={`${deviceName(item.device_id)} — ${formatTime(item.ts)}${item.batch_status === "failed" ? " ⚠ Unverified" : ""}`}
                  >
                    <LogImage
                      imageBytes={image}
                      previewTitle={previewTitle}
                      previewSubtitle={previewSubtitle}
                    />
                  </div>
                );
              })}
            </div>
          </section>
        ))}
      </div>
      {hasMore && <div class="logs-load-sentinel" ref={loadSentinelRef} />}
      {loading && <p class="logs-loading">Loading…</p>}
    </>
  );
}
