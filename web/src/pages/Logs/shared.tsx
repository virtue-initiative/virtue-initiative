import { useState, useEffect } from "preact/hooks";
import { BatchVerification } from "../../crypto";
import { formatDayHeading, localDateKey } from "../../utils/time";

export interface LogItem {
  id: string;
  taken_at: number; // ms epoch
  device_id: string;
  kind: string;
  image?: Uint8Array;
  metadata: [string, string][];
  batch_status: BatchVerification;
  source?: "batch" | "log";
}

export type ImageLogItem = LogItem & { image: Uint8Array };

export interface LogDayGroup<T extends { taken_at: number }> {
  key: string;
  label: string;
  items: T[];
}

export function groupLogsByDay<T extends { taken_at: number }>(
  items: T[],
): LogDayGroup<T>[] {
  const groups: LogDayGroup<T>[] = [];
  const byKey = new Map<string, LogDayGroup<T>>();

  for (const item of items) {
    const key = localDateKey(item.taken_at);
    let group = byKey.get(key);
    if (!group) {
      group = {
        key,
        label: formatDayHeading(item.taken_at),
        items: [],
      };
      byKey.set(key, group);
      groups.push(group);
    }
    group.items.push(item);
  }

  return groups;
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
        <img class="log-thumb" src={imgSrc} alt="screenshot" loading="lazy" />
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
