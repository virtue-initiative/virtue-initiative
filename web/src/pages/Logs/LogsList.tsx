import { Fragment } from "preact";
import { LogItem, LogImage } from "./shared";

function humanizeKind(kind: string): string {
  return kind.replace(/_/g, " ");
}

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
              {item.image ? (
                <LogImage imageBytes={item.image} />
              ) : (
                <div class="log-thumb-status">No image</div>
              )}
            </div>
            <div class="log-row-main">
              <div class="log-row-top">
                <span class="log-type">{humanizeKind(item.kind)}</span>
                <span class="log-device">{deviceName(item.device_id)}</span>
                {item.batch_status === "failed" && (
                  <span
                    class="verify-badge verify-badge--failed"
                    title="Batch hash chain verification failed — data may have been tampered with"
                  >
                    ⚠ Unverified
                  </span>
                )}
                <span class="log-time">
                  {new Date(item.taken_at).toLocaleTimeString([], {
                    hour: "2-digit",
                    minute: "2-digit",
                    second: "2-digit",
                  })}
                </span>
              </div>
              <p class="log-device" style="margin:0.15rem 0 0.4rem;">
                {new Date(item.taken_at).toLocaleDateString()}
              </p>
              {item.metadata.length > 0 && (
                <dl class="log-meta">
                  {item.metadata.map(([key, value], index) => (
                    <Fragment key={`${item.id}-meta-${index}`}>
                      <dt>{key}</dt>
                      <dd>{value}</dd>
                    </Fragment>
                  ))}
                </dl>
              )}
            </div>
          </div>
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
