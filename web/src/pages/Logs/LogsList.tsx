import { Fragment } from "preact";
import { groupLogsByDay, LogItem, LogImage } from "./shared";
import { formatRelativeTimestamp, formatTime } from "../../utils/time";

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
  const dayGroups = groupLogsByDay(items);

  return (
    <>
      <div class="section-stack">
        {dayGroups.map((group) => (
          <section class="logs-day-group" key={group.key}>
            <h2 class="section-heading">{group.label}</h2>
            <div class="log-list">
              {group.items.map((item) => (
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
                      <span class="log-device">
                        {deviceName(item.device_id)}
                      </span>
                      {item.source === "log" && (
                        <span
                          class="verify-badge verify-badge--alert"
                          title="Immediate alert log"
                        >
                          ⚡ Alert
                        </span>
                      )}
                      {item.batch_status === "failed" && (
                        <span
                          class="verify-badge verify-badge--failed"
                          title="Batch hash chain verification failed — data may have been tampered with"
                        >
                          ⚠ Unverified
                        </span>
                      )}
                      <span class="log-time" title={formatTime(item.taken_at)}>
                        {formatRelativeTimestamp(item.taken_at)}
                      </span>
                    </div>
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
