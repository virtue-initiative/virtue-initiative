import { useEffect, useState } from "preact/hooks";
import { DataLog } from "../../api";
import { BatchVerification } from "../../crypto";
import { formatDayHeading, localDateKey } from "../../utils/time";

export type FeedLog = DataLog & {
  batch_status: BatchVerification;
  source: "batch" | "log";
};

export interface LogDayGroup<T extends { ts: number }> {
  key: string;
  label: string;
  items: T[];
}

export function groupLogsByDay<T extends { ts: number }>(
  items: T[],
): LogDayGroup<T>[] {
  const groups: LogDayGroup<T>[] = [];
  const byKey = new Map<string, LogDayGroup<T>>();

  for (const item of items) {
    const key = localDateKey(item.ts);
    let group = byKey.get(key);
    if (!group) {
      group = {
        key,
        label: formatDayHeading(item.ts),
        items: [],
      };
      byKey.set(key, group);
      groups.push(group);
    }
    group.items.push(item);
  }

  return groups;
}

export function toUint8Array(value: unknown): Uint8Array | undefined {
  if (!value) return undefined;
  if (value instanceof Uint8Array) return value;
  if (Array.isArray(value)) return new Uint8Array(value as number[]);
  if (typeof value === "string") {
    try {
      return Uint8Array.fromBase64(value);
    } catch {
      return undefined;
    }
  }
  return undefined;
}

export function getLogImage(log: DataLog): Uint8Array | undefined {
  return toUint8Array(log.data.image);
}

export function getLogMetadata(log: DataLog) {
  return Object.entries(log.data)
    .filter(([key]) => key !== "image")
    .map(
      ([key, value]) =>
        [key, typeof value === "string" ? value : JSON.stringify(value)] as [
          string,
          string,
        ],
    );
}

export function LogImage({ imageBytes }: { imageBytes: Uint8Array }) {
  const [imgSrc, setImgSrc] = useState<string | null>(null);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const imageData = Uint8Array.from(imageBytes);
    const url = URL.createObjectURL(
      new Blob([imageData], { type: "image/webp" }),
    );
    setImgSrc(url);
    return () => URL.revokeObjectURL(url);
  }, [imageBytes]);

  if (!imgSrc) return null;

  return (
    <>
      <button
        class="log-thumb-btn"
        type="button"
        onClick={() => setOpen(true)}
        aria-label="View screenshot"
      >
        <img
          class="log-thumb"
          src={imgSrc}
          alt="screenshot"
          loading="lazy"
          onLoad={(event) => {
            if (!onDimensions) {
              return;
            }

            const image = event.currentTarget;
            if (image.naturalWidth > 0 && image.naturalHeight > 0) {
              onDimensions(image.naturalWidth, image.naturalHeight);
            }
          }}
        />
      </button>
      {open && (
        <div class="img-overlay" onClick={() => setOpen(false)}>
          <div class="img-full-frame">
            <img
              class="img-full"
              src={imgSrc}
              alt="screenshot"
              onClick={(e) => e.stopPropagation()}
            />
          </div>
        </div>
      )}
    </>
  );
}
