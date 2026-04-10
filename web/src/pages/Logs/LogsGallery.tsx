import { useState } from "preact/hooks";
import { formatTime } from "../../utils/time";
import { FeedLog, getLogImage, groupLogsByDay, LogImage } from "./shared";

const GALLERY_THUMB_SIZE = 72;
const GALLERY_FULLSCREEN_THUMB_SIZE = 96;
const DEVICE_ASPECT_RATIO_TOLERANCE = 0.12;
const MIN_DEVICE_ASPECT_RATIO = 0.6;
const MAX_DEVICE_ASPECT_RATIO = 1.9;

function clampAspectRatio(aspectRatio: number): number {
  return Math.min(
    MAX_DEVICE_ASPECT_RATIO,
    Math.max(MIN_DEVICE_ASPECT_RATIO, aspectRatio),
  );
}

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
  const [imageAspectsById, setImageAspectsById] = useState<
    Record<string, number>
  >({});
  const [deviceAspectsById, setDeviceAspectsById] = useState<
    Record<string, number>
  >({});

  if (items.length === 0 && !loading) {
    return <p class="empty">No screenshots found.</p>;
  }
  const dayGroups = groupLogsByDay(items);
  const thumbnailSize = fullscreen
    ? GALLERY_FULLSCREEN_THUMB_SIZE
    : GALLERY_THUMB_SIZE;

  function registerAspectRatio(item: FeedLog, width: number, height: number) {
    if (height <= 0 || width <= 0) {
      return;
    }

    const aspectRatio = clampAspectRatio(width / height);
    setImageAspectsById((current) => {
      if (current[item.id] === aspectRatio) {
        return current;
      }

      return { ...current, [item.id]: aspectRatio };
    });

    setDeviceAspectsById((current) => {
      if (current[item.device_id] !== undefined) {
        return current;
      }

      return { ...current, [item.device_id]: aspectRatio };
    });
  }

  function thumbnailWidth(item: FeedLog): number {
    const itemAspect = imageAspectsById[item.id];
    if (itemAspect === undefined) {
      return thumbnailSize;
    }

    const deviceAspect = deviceAspectsById[item.device_id];
    if (deviceAspect === undefined) {
      return Math.round(thumbnailSize * itemAspect);
    }

    const relativeDifference =
      Math.abs(itemAspect - deviceAspect) / deviceAspect;
    if (relativeDifference > DEVICE_ASPECT_RATIO_TOLERANCE) {
      return thumbnailSize;
    }

    return Math.round(thumbnailSize * deviceAspect);
  }

  return (
    <>
      <div class="section-stack">
        {dayGroups.map((group) => (
          <section class="logs-day-group gallery-day-group" key={group.key}>
            <h2 class="section-heading">{group.label}</h2>
            <div
              class={`gallery-grid${fullscreen ? " gallery-grid--fullscreen" : ""}`}
            >
              {group.items.map((item) => {
                const image = getLogImage(item);
                if (!image) {
                  return null;
                }

                return (
                  <div
                    class={`gallery-item${item.batch_status === "failed" ? " gallery-item--unverified" : ""}`}
                    key={item.id}
                    title={`${deviceName(item.device_id)} — ${formatTime(item.ts)}${item.batch_status === "failed" ? " ⚠ Unverified" : ""}`}
                    style={{
                      width: `${thumbnailWidth(item)}px`,
                      height: `${thumbnailSize}px`,
                    }}
                  >
                    <LogImage
                      imageBytes={image}
                      onDimensions={(width, height) =>
                        registerAspectRatio(item, width, height)
                      }
                    />
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
