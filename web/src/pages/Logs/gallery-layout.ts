import { FeedLog } from "./shared";

export interface GalleryRow {
  /** Index of first item in the source array (inclusive). */
  startIndex: number;
  /** Number of items in this row. */
  count: number;
  /** Final row height in px (after scaling). */
  height: number;
  /** Final widths per item, length === count, sum + gaps === containerWidth (except last partial row). */
  widths: number[];
}

export interface GalleryLayoutOptions {
  containerWidth: number;
  targetRowHeight: number;
  gap: number;
  defaultRatio: number;
  minRowScale: number;
  maxLastRowScale: number;
}

function ratioOf(item: FeedLog, defaultRatio: number): number {
  if (
    typeof item.image_w === "number" &&
    typeof item.image_h === "number" &&
    item.image_w > 0 &&
    item.image_h > 0
  ) {
    return item.image_w / item.image_h;
  }
  return defaultRatio;
}

export function buildGalleryRows(
  items: FeedLog[],
  options: GalleryLayoutOptions,
): GalleryRow[] {
  const {
    containerWidth: W,
    targetRowHeight: H,
    gap: G,
    defaultRatio,
    minRowScale,
    maxLastRowScale,
  } = options;

  if (W <= 0 || items.length === 0) return [];

  const rows: GalleryRow[] = [];
  let i = 0;

  while (i < items.length) {
    const start = i;
    let totalRatio = 0;
    const rowItems: FeedLog[] = [];

    while (i < items.length) {
      const r = ratioOf(items[i], defaultRatio);
      const tentativeRatio = totalRatio + r;
      const tentativeCount = rowItems.length + 1;
      const usableW = W - G * (tentativeCount - 1);
      const tentativeScaledH = usableW / tentativeRatio;

      if (rowItems.length > 0 && tentativeScaledH < H * minRowScale) {
        break;
      }

      rowItems.push(items[i]);
      totalRatio = tentativeRatio;
      i++;
    }

    const count = rowItems.length;
    const usableW = W - G * (count - 1);
    const scaledH = usableW / totalRatio;

    const isLast = i >= items.length;
    const rowH = isLast ? Math.min(scaledH, H * maxLastRowScale) : scaledH;

    const widths = rowItems.map((it) => rowH * ratioOf(it, defaultRatio));

    rows.push({ startIndex: start, count, height: rowH, widths });
  }

  return rows;
}
