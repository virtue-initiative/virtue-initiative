import { useCallback, useEffect, useMemo, useRef, useState } from 'preact/hooks';
import {
  useVirtualizer,
  observeElementRect,
  observeElementOffset,
  elementScroll,
} from '@tanstack/react-virtual';
import { formatRelativeTimestamp } from '../../utils/time';
import {
  EventImage,
  FeedLog,
  formatDayAndTime,
  getLogCategory,
  getLogMessage,
  LogDetailDialog,
  LogIcon,
} from './shared';
import { buildGalleryRows } from './gallery-layout';
import { getRiskLevel } from '@virtueinitiative/shared-web/risk';

/** Badge text per concern level, matching the list view's row badges. */
const RISK_BADGES = {
  alert: '⚠ Alert',
  high: '⚠ High',
  medium: 'Med',
} as const;

const TARGET_ROW_HEIGHT = 140;
const GAP = 8;
const DEFAULT_RATIO = 16 / 9;
const MIN_ROW_SCALE = 0.6;
const MAX_LAST_ROW_SCALE = 1.0;

export function LogsGallery({
  items,
  loading,
  deviceName,
  onVisibleDateChange,
  viewerId,
}: {
  items: FeedLog[];
  loading: boolean;
  hasMore: boolean;
  onLoadMore: () => void;
  deviceName: (id: string) => string;
  onVisibleDateChange?: (date: string | null) => void;
  viewerId: string;
}) {
  const [wrapperEl, setWrapperEl] = useState<HTMLDivElement | null>(null);
  // Logs stream in while the dialog is open, so an id survives where a stored
  // index or object wouldn't.
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const wrapperRef = useCallback((el: HTMLDivElement | null) => setWrapperEl(el), []);
  const [containerWidth, setContainerWidth] = useState(0);
  const rafRef = useRef<number | null>(null);

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

  const gap = GAP;

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

  const scrollMargin = wrapperEl?.offsetTop ?? 0;

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => (wrapperEl?.closest('.logs-main') as HTMLElement | null) ?? wrapperEl,
    observeElementRect,
    observeElementOffset,
    scrollToFn: elementScroll,
    scrollMargin,
    estimateSize: (index) => rows[index].height + gap,
    overscan: 3,
    getItemKey: (index) =>
      items[rows[index].startIndex]?.id ?? `${rows[index].startIndex}-${rows[index].count}`,
    useAnimationFrameWithResizeObserver: true,
    onChange: (instance) => {
      if (!onVisibleDateChange) return;
      const firstRow = instance.getVirtualItems()[0];
      const row = firstRow ? rows[firstRow.index] : null;
      const item = row ? items[row.startIndex] : null;
      onVisibleDateChange(item ? formatDayAndTime(item.ts) : null);
    },
  });

  useEffect(() => {
    virtualizer.measure();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows]);

  const selectedIndex = selectedId === null ? -1 : items.findIndex((i) => i.id === selectedId);

  if (items.length === 0 && !loading) {
    return <p class="empty">No logs found.</p>;
  }

  return (
    <div class="logs-gallery-virtual" ref={wrapperRef}>
      <div
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          width: '100%',
          position: 'relative',
        }}
      >
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const row = rows[virtualRow.index];
          const rowItems = items.slice(row.startIndex, row.startIndex + row.count);
          return (
            <div
              key={virtualRow.key}
              style={{
                position: 'absolute',
                top: `${virtualRow.start - virtualizer.options.scrollMargin}px`,
                left: 0,
                width: '100%',
                height: `${row.height}px`,
                display: 'flex',
                gap: `${gap}px`,
                flexWrap: 'nowrap',
              }}
            >
              {rowItems.map((item, k) => {
                const cellWidth = row.widths[k];
                const riskLevel = getRiskLevel(item.risk);
                return (
                  <div
                    key={item.id}
                    class={`logs-gallery-item${item.batch_status === 'failed' ? ' logs-gallery-item--unverified' : ''}${riskLevel === 'low' ? '' : ` logs-gallery-item--risk-${riskLevel}`}`}
                    style={{
                      width: `${cellWidth}px`,
                      height: `${row.height}px`,
                      flexShrink: 0,
                    }}
                  >
                    {/* Only the image tiles need the badge — a card already
                        spells out its category and message in text. */}
                    {riskLevel !== 'low' && item.image_w !== undefined && (
                      <span
                        class={`logs-verify-badge logs-gallery-risk-badge logs-verify-badge--${
                          riskLevel === 'medium' ? 'moderate' : 'failed'
                        }`}
                      >
                        {RISK_BADGES[riskLevel]}
                      </span>
                    )}
                    {item.image_w !== undefined ? (
                      <EventImage
                        eventId={item.id}
                        viewerId={viewerId}
                        onClick={() => setSelectedId(item.id)}
                      />
                    ) : (
                      <button
                        class="logs-gallery-card"
                        type="button"
                        onClick={() => setSelectedId(item.id)}
                      >
                        <span class="logs-gallery-card-icon">
                          <LogIcon log={item} />
                        </span>
                        <span class="logs-gallery-card-type">{getLogCategory(item)}</span>
                        <span class="logs-gallery-card-message">
                          {getLogMessage(item, deviceName(item.device_id))}
                        </span>
                        <span class="logs-gallery-card-time">
                          {formatRelativeTimestamp(item.ts)}
                        </span>
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>
      {loading && <p class="logs-loading">Loading…</p>}
      {selectedIndex !== -1 && (
        <LogDetailDialog
          item={items[selectedIndex]}
          deviceName={deviceName}
          onClose={() => setSelectedId(null)}
          viewerId={viewerId}
          onPrev={selectedIndex > 0 ? () => setSelectedId(items[selectedIndex - 1].id) : undefined}
          onNext={
            selectedIndex < items.length - 1
              ? () => setSelectedId(items[selectedIndex + 1].id)
              : undefined
          }
        />
      )}
    </div>
  );
}
