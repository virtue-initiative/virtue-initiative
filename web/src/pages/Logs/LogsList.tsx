import { useRef, useState } from 'preact/hooks';
import { useVirtualizer } from '@tanstack/react-virtual';
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
import { getRiskLevel } from '@virtueinitiative/shared-web/risk';

const ITEM_HEIGHT = 68;

export function LogsList({
  items,
  loading,
  hasMore,
  onLoadMore,
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
  // Logs stream in while the dialog is open, so an id survives where a stored
  // index or object wouldn't.
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () =>
      (scrollRef.current?.closest('.logs-main') as HTMLElement | null) ?? scrollRef.current,
    scrollMargin: scrollRef.current?.offsetTop ?? 0,
    estimateSize: () => ITEM_HEIGHT,
    overscan: 10,
    useAnimationFrameWithResizeObserver: true,
    onChange: (instance) => {
      const virtualItems = instance.getVirtualItems();
      if (hasMore && !loading) {
        const lastItem = virtualItems[virtualItems.length - 1];
        if (lastItem && lastItem.index >= items.length - 1) {
          onLoadMore();
        }
      }
      if (onVisibleDateChange) {
        const firstVisible = virtualItems[0];
        const item = firstVisible ? items[firstVisible.index] : null;
        onVisibleDateChange(item ? formatDayAndTime(item.ts) : null);
      }
    },
  });

  const selectedIndex = selectedId === null ? -1 : items.findIndex((i) => i.id === selectedId);

  if (items.length === 0 && !loading) {
    return <p class="empty">No logs found.</p>;
  }

  return (
    <>
      <div class="logs-virtual-scroll" ref={scrollRef}>
        <div
          class="logs-virtual-scroll-inner"
          style={{
            height: `${virtualizer.getTotalSize()}px`,
            width: '100%',
            position: 'relative',
          }}
        >
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const item = items[virtualRow.index];
            const isFirst = virtualRow.index === 0;
            const isLast = virtualRow.index === items.length - 1;

            return (
              <button
                key={item.id}
                class={`logs-vrow${isFirst ? ' logs-vrow--first' : ''}${isLast ? ' logs-vrow--last' : ''}`}
                type="button"
                style={{
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  width: '100%',
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${virtualRow.start - virtualizer.options.scrollMargin}px)`,
                }}
                onClick={() => setSelectedId(item.id)}
              >
                <div class="logs-vrow-thumb">
                  {item.image_w !== undefined ? (
                    <EventImage eventId={item.id} viewerId={viewerId} />
                  ) : (
                    <div class="logs-thumb-placeholder logs-thumb-icon">
                      <LogIcon log={item} />
                    </div>
                  )}
                </div>
                <div class="logs-vrow-body">
                  <div class="logs-vrow-top">
                    <span class="logs-type">{getLogCategory(item)}</span>
                    <span class="logs-device">{deviceName(item.device_id)}</span>
                    {getRiskLevel(item.risk) === 'alert' && (
                      <span class="logs-verify-badge logs-verify-badge--failed">⚠ Alert</span>
                    )}
                    {getRiskLevel(item.risk) === 'high' && (
                      <span class="logs-verify-badge logs-verify-badge--failed">⚠ High</span>
                    )}
                    {getRiskLevel(item.risk) === 'medium' && (
                      <span class="logs-verify-badge logs-verify-badge--moderate">Med</span>
                    )}
                    {item.batch_status === 'failed' && (
                      <span class="logs-verify-badge logs-verify-badge--failed">⚠ Unverified</span>
                    )}
                  </div>
                  <div class="logs-vrow-sub">
                    <span class="logs-vrow-message">
                      {getLogMessage(item, deviceName(item.device_id))}
                    </span>
                    <span class="logs-time">{formatRelativeTimestamp(item.ts)}</span>
                  </div>
                </div>
              </button>
            );
          })}
        </div>
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
    </>
  );
}
