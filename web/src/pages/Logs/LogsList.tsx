import { Fragment } from "preact";
import { useEffect, useRef } from "preact/hooks";
import {
  formatDate,
  formatRelativeTimestamp,
  formatTime,
} from "../../utils/time";
import {
  describeRiskLevel,
  FeedLog,
  getLogImage,
  getLogMetadata,
  humanizeLogType,
  LogImage,
} from "./shared";

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

  if (items.length === 0 && !loading) {
    return <p class="empty">No logs found.</p>;
  }

  if (items.length === 0 && !loading) {
    return <p class="empty">No logs found.</p>;
  }

  return (
    <>
      <div class="logs-list">
        {items.map((item) => {
          const image = getLogImage(item);
          const metadata = getLogMetadata(item);
          const riskLabel = describeRiskLevel(item.risk) ?? "Risk unavailable";
          const previewTitle = `${formatDate(item.ts)} ${formatTime(item.ts)}`;
          const previewSubtitle = `${riskLabel}${item.batch_status === "failed" ? " • Unverified" : ""}`;

          return (
            <div class="logs-row" key={item.id}>
              <div class="logs-thumb-wrap">
                {image ? (
                  <LogImage
                    imageBytes={image}
                    previewTitle={previewTitle}
                    previewSubtitle={previewSubtitle}
                  />
                ) : (
                  <div class="logs-thumb-status">No image</div>
                )}
              </div>
              <div class="logs-row-main">
                <div class="logs-row-top">
                  <span class="logs-type">{humanizeLogType(item.type)}</span>
                  <span class="logs-device">{deviceName(item.device_id)}</span>
                  {item.risk > 0.7 ? (
                    <span
                      class="logs-verify-badge logs-verify-badge--failed"
                      title="High risk log"
                    >
                      ⚠ High risk
                    </span>
                  ) : (
                    item.risk > 0.4 && (
                      <span
                        class="logs-verify-badge logs-verify-badge--moderate"
                        title="Moderate risk log"
                      >
                        Moderate risk
                      </span>
                    )
                  )}
                  {item.batch_status === "failed" && (
                    <span
                      class="logs-verify-badge logs-verify-badge--failed"
                      title="Batch hash chain verification failed — data may have been tampered with"
                    >
                      ⚠ Unverified
                    </span>
                  )}
                  <span class="logs-time" title={formatTime(item.ts)}>
                    {formatRelativeTimestamp(item.ts)}
                  </span>
                </div>
                {metadata.length > 0 && (
                  <dl class="logs-meta">
                    {metadata.map(([key, value], index) => (
                      <Fragment key={`${item.id}-meta-${index}`}>
                        <dt>{key}</dt>
                        <dd>{value}</dd>
                      </Fragment>
                    ))}
                  </dl>
                )}
              </div>
            </div>
          );
        })}
      </div>
      {hasMore && <div class="logs-load-sentinel" ref={loadSentinelRef} />}
      {loading && <p class="logs-loading">Loading…</p>}
    </>
  );
}
