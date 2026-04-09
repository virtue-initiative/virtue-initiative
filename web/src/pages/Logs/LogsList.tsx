import { Fragment } from "preact";
import { formatRelativeTimestamp, formatTime } from "../../utils/time";
import {
  FeedLog,
  getLogImage,
  getLogMetadata,
  groupLogsByDay,
  LogImage,
} from "./shared";

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
  items: FeedLog[];
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
              {group.items.map((item) => {
                const image = getLogImage(item);
                const metadata = getLogMetadata(item);

                return (
                  <div class="log-row" key={item.id}>
                    <div class="log-thumb-wrap">
                      {image ? (
                        <LogImage imageBytes={image} />
                      ) : (
                        <div class="log-thumb-status">No image</div>
                      )}
                    </div>
                    <div class="log-row-main">
                      <div class="log-row-top">
                        <span class="log-type">{humanizeKind(item.type)}</span>
                        <span class="log-device">
                          {deviceName(item.device_id)}
                        </span>
                        {item.risk > 0.7 ? (
                          <span
                            class="verify-badge verify-badge--failed"
                            title="High risk log"
                          >
                            ⚠ High Risk
                          </span>
                        ) : (
                          item.risk > 0.4 && (
                            <span
                              class="verify-badge verify-badge--moderate"
                              title="Moderate risk log"
                            >
                              Moderate Risk
                            </span>
                          )
                        )}
                        {item.batch_status === "failed" && (
                          <span
                            class="verify-badge verify-badge--failed"
                            title="Batch hash chain verification failed — data may have been tampered with"
                          >
                            ⚠ Unverified
                          </span>
                        )}
                        <span class="log-time" title={formatTime(item.ts)}>
                          {formatRelativeTimestamp(item.ts)}
                        </span>
                      </div>
                      {metadata.length > 0 && (
                        <dl class="log-meta">
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
