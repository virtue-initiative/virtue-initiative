import { useCallback, useEffect, useMemo, useState } from "preact/hooks";
import { useVirtualizer } from "@tanstack/react-virtual";
import { formatDate, formatTime } from "../../utils/time";
import { describeRiskLevel, FeedLog, getLogImage, LogImage } from "./shared";
import { buildGalleryRows } from "./gallery-layout";

const TARGET_ROW_HEIGHT = 140;
const GAP_NORMAL = 8;
const GAP_FULLSCREEN = 10;
const DEFAULT_RATIO = 16 / 9;
const MIN_ROW_SCALE = 0.6;
const MAX_LAST_ROW_SCALE = 1.0;

export function LogsGallery({
  items,
  loading,
  fullscreen,
  deviceName,
}: {
  items: FeedLog[];
  loading: boolean;
  hasMore: boolean;
  onLoadMore: () => void;
  deviceName: (id: string) => string;
  fullscreen: boolean;
}) {
  const [wrapperEl, setWrapperEl] = useState<HTMLDivElement | null>(null);
  const wrapperRef = useCallback((el: HTMLDivElement | null) => setWrapperEl(el), []);
  const [containerWidth, setContainerWidth] = useState(0);

  useEffect(() => {
    if (!wrapperEl) return;
    setContainerWidth(wrapperEl.getBoundingClientRect().width);
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) setContainerWidth(entry.contentRect.width);
    });
    ro.observe(wrapperEl);
    return () => ro.disconnect();
  }, [wrapperEl]);

  const gap = fullscreen ? GAP_FULLSCREEN : GAP_NORMAL;

  const rows = useMemo(
    () =>
      buildGalleryRows(items, {
        containerWidth,
        targetRowHeight: TARGET_ROW_HEIGHT,
        gap,
        defaultRatio: DEFAULT_RATIO,
        minRowScale: MIN_ROW_SCALE,
        maxLastRowScale: MAX_LAST_ROW_SCALE,
      }),
    [items, containerWidth, gap],
  );

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () =>
      (wrapperEl?.closest(".logs-main") as HTMLElement | null) ?? wrapperEl,
    scrollMargin: wrapperEl?.offsetTop ?? 0,
    estimateSize: (index) => rows[index].height + gap,
    overscan: 3,
    getItemKey: (index) => `${rows[index].startIndex}-${rows[index].count}`,
  });

  useEffect(() => {
    virtualizer.measure();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows]);

  if (items.length === 0 && !loading) {
    return <p class="empty">No screenshots found.</p>;
  }

  return (
    <div class="logs-gallery-virtual" ref={wrapperRef}>
      <div
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          width: "100%",
          position: "relative",
        }}
      >
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const row = rows[virtualRow.index];
          const rowItems = items.slice(
            row.startIndex,
            row.startIndex + row.count,
          );
          return (
            <div
              key={virtualRow.key}
              style={{
                position: "absolute",
                top: `${virtualRow.start - virtualizer.options.scrollMargin}px`,
                left: 0,
                width: "100%",
                height: `${row.height}px`,
                display: "flex",
                gap: `${gap}px`,
                flexWrap: "nowrap",
              }}
            >
              {rowItems.map((item, k) => {
                const image = getLogImage(item);
                if (!image) return null;
                const cellWidth = row.widths[k];
                const riskLabel =
                  describeRiskLevel(item.risk) ?? "Risk unavailable";
                const previewTitle = `${formatDate(item.ts)} ${formatTime(item.ts)}`;
                const previewSubtitle = `${riskLabel}${item.batch_status === "failed" ? " • Unverified" : ""}`;
                return (
                  <div
                    key={item.id}
                    class={`logs-gallery-item${item.batch_status === "failed" ? " logs-gallery-item--unverified" : ""}`}
                    style={{
                      width: `${cellWidth}px`,
                      height: `${row.height}px`,
                      flexShrink: 0,
                    }}
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
          );
        })}
      </div>
      {loading && <p class="logs-loading">Loading…</p>}
    </div>
  );
}
