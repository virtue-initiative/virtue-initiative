import { useCallback, useEffect, useMemo, useRef, useState } from "preact/hooks";
import {
  useVirtualizer,
  observeElementRect,
  observeElementOffset,
  elementScroll,
  observeWindowRect,
  observeWindowOffset,
  windowScroll,
} from "@tanstack/react-virtual";
import { formatDate, formatTime } from "../../utils/time";
import { describeRiskLevel, FeedLog, getLogImage, LogImage } from "./shared";
import { buildGalleryRows } from "./gallery-layout";

const TARGET_ROW_HEIGHT = 140;
const GAP_NORMAL = 8;
const GAP_FULLSCREEN = 10;
const DEFAULT_RATIO = 16 / 9;
const MIN_ROW_SCALE = 0.6;
const MAX_LAST_ROW_SCALE = 1.0;

function useIsNarrowViewport() {
  const [isNarrow, setIsNarrow] = useState(
    () => typeof window !== "undefined" && window.matchMedia("(max-width: 720px)").matches,
  );
  useEffect(() => {
    const mq = window.matchMedia("(max-width: 720px)");
    const handler = (e: MediaQueryListEvent) => setIsNarrow(e.matches);
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);
  return isNarrow;
}

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
  const rafRef = useRef<number | null>(null);
  const isNarrow = useIsNarrowViewport();

  useEffect(() => {
    if (!wrapperEl) return;
    setContainerWidth(Math.round(wrapperEl.getBoundingClientRect().width));
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
      const width = Math.round(entry.contentRect.width);
      rafRef.current = requestAnimationFrame(() => {
        rafRef.current = null;
        setContainerWidth((prev) => (prev === width ? prev : width));
      });
    });
    ro.observe(wrapperEl);
    return () => {
      ro.disconnect();
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
    };
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

  const scrollMargin = isNarrow
    ? (wrapperEl ? wrapperEl.getBoundingClientRect().top + window.scrollY : 0)
    : (wrapperEl?.offsetTop ?? 0);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () =>
      isNarrow
        ? (typeof window !== "undefined" ? (window as unknown as HTMLElement) : null)
        : ((wrapperEl?.closest(".logs-main") as HTMLElement | null) ?? wrapperEl),
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    observeElementRect: (isNarrow ? observeWindowRect : observeElementRect) as any,
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    observeElementOffset: (isNarrow ? observeWindowOffset : observeElementOffset) as any,
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    scrollToFn: (isNarrow ? windowScroll : elementScroll) as any,
    scrollMargin,
    estimateSize: (index) => rows[index].height + gap,
    overscan: 3,
    getItemKey: (index) => items[rows[index].startIndex]?.id ?? `${rows[index].startIndex}-${rows[index].count}`,
    useAnimationFrameWithResizeObserver: true,
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
