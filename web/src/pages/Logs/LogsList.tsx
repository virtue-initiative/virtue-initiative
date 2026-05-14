import { useEffect, useRef, useState } from "preact/hooks";
import { useVirtualizer } from "@tanstack/react-virtual";
import { formatRelativeTimestamp } from "../../utils/time";
import {
  FeedLog,
  getLogImage,
  humanizeLogType,
  LogDetailDialog,
} from "./shared";

const ITEM_HEIGHT = 68;

function ThumbImage({ imageBytes }: { imageBytes: Uint8Array }) {
  const [src, setSrc] = useState<string | null>(null);

  useEffect(() => {
    const url = URL.createObjectURL(
      new Blob(
        [
          imageBytes.buffer.slice(
            imageBytes.byteOffset,
            imageBytes.byteOffset + imageBytes.byteLength,
          ) as ArrayBuffer,
        ],
        { type: "image/webp" },
      ),
    );
    setSrc(url);
    return () => URL.revokeObjectURL(url);
  }, [imageBytes]);

  if (!src) return <div class="logs-thumb-placeholder" />;
  return <img class="logs-thumb-image" src={src} alt="" loading="lazy" />;
}


export function LogsList({
  items,
  loading,
  hasMore,
  onLoadMore,
  deviceName,
}: {
  items: FeedLog[];
  loading: boolean;
  hasMore: boolean;
  onLoadMore: () => void;
  deviceName: (id: string) => string;
}) {
  const [selectedItem, setSelectedItem] = useState<FeedLog | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () =>
      (scrollRef.current?.closest(".logs-main") as HTMLElement | null) ??
      scrollRef.current,
    scrollMargin: scrollRef.current?.offsetTop ?? 0,
    estimateSize: () => ITEM_HEIGHT,
    overscan: 10,
    useAnimationFrameWithResizeObserver: true,
    onChange: (instance) => {
      if (!hasMore || loading) return;
      const virtualItems = instance.getVirtualItems();
      const lastItem = virtualItems[virtualItems.length - 1];
      if (lastItem && lastItem.index >= items.length - 1) {
        onLoadMore();
      }
    },
  });

  if (items.length === 0 && !loading) {
    return <p class="empty">No logs found.</p>;
  }

  return (
    <>
      <div class="logs-virtual-scroll" ref={scrollRef}>
        <div
          style={{
            height: `${virtualizer.getTotalSize()}px`,
            width: "100%",
            position: "relative",
          }}
        >
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const item = items[virtualRow.index];
            const image = getLogImage(item);

            return (
              <button
                key={item.id}
                class="logs-vrow"
                type="button"
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${virtualRow.start - virtualizer.options.scrollMargin}px)`,
                }}
                onClick={() => setSelectedItem(item)}
              >
                <div class="logs-vrow-thumb">
                  {image ? (
                    <ThumbImage imageBytes={image} />
                  ) : (
                    <div class="logs-thumb-placeholder" />
                  )}
                </div>
                <div class="logs-vrow-body">
                  <div class="logs-vrow-top">
                    <span class="logs-type">{humanizeLogType(item.type)}</span>
                    <span class="logs-device">
                      {deviceName(item.device_id)}
                    </span>
                    {item.risk > 0.7 && (
                      <span class="logs-verify-badge logs-verify-badge--failed">
                        ⚠ High
                      </span>
                    )}
                    {item.risk > 0.4 && item.risk <= 0.7 && (
                      <span class="logs-verify-badge logs-verify-badge--moderate">
                        Med
                      </span>
                    )}
                    {item.batch_status === "failed" && (
                      <span class="logs-verify-badge logs-verify-badge--failed">
                        ⚠ Unverified
                      </span>
                    )}
                  </div>
                  <div class="logs-vrow-sub">
                    <span class="logs-time">
                      {formatRelativeTimestamp(item.ts)}
                    </span>
                  </div>
                </div>
              </button>
            );
          })}
        </div>
      </div>
      {loading && <p class="logs-loading">Loading…</p>}
      {selectedItem && (
        <LogDetailDialog
          item={selectedItem}
          deviceName={deviceName}
          onClose={() => setSelectedItem(null)}
        />
      )}
    </>
  );
}
