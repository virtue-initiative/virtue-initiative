import { describe, expect, it } from "vitest";
import { buildGalleryRows } from "./gallery-layout";
import { FeedLog } from "./shared";

function makeItem(overrides: Partial<FeedLog> = {}): FeedLog {
  return {
    id: "x",
    device_id: "d",
    ts: 0,
    type: "screenshot",
    data: {},
    created_at: 0,
    risk: undefined,
    batch_status: "unknown",
    source: "batch",
    ...overrides,
  } as FeedLog;
}

function make169(n: number): FeedLog[] {
  return Array.from({ length: n }, (_, i) =>
    makeItem({ id: String(i), image_w: 1920, image_h: 1080 }),
  );
}

function makePortrait(n: number): FeedLog[] {
  return Array.from({ length: n }, (_, i) =>
    makeItem({ id: String(i), image_w: 1080, image_h: 1920 }),
  );
}

const OPTS = {
  containerWidth: 1000,
  targetRowHeight: 140,
  gap: 8,
  defaultRatio: 16 / 9,
  minRowScale: 0.6,
  maxLastRowScale: 1.0,
};

describe("buildGalleryRows", () => {
  it("returns [] for empty items", () => {
    expect(buildGalleryRows([], OPTS)).toEqual([]);
  });

  it("returns [] when containerWidth is 0", () => {
    expect(buildGalleryRows(make169(5), { ...OPTS, containerWidth: 0 })).toEqual([]);
  });

  it("all 16:9 items — widths sum to containerWidth (non-last rows)", () => {
    const items = make169(10);
    const rows = buildGalleryRows(items, OPTS);
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows.slice(0, -1)) {
      const widthSum = row.widths.reduce((a, b) => a + b, 0);
      const total = widthSum + OPTS.gap * (row.count - 1);
      expect(total).toBeCloseTo(OPTS.containerWidth, 1);
    }
  });

  it("portrait items — more items per row than landscape", () => {
    const landscape = buildGalleryRows(make169(20), OPTS);
    const portrait = buildGalleryRows(makePortrait(20), OPTS);
    const avgLandscape = landscape[0]?.count ?? 0;
    const avgPortrait = portrait[0]?.count ?? 0;
    expect(avgPortrait).toBeGreaterThan(avgLandscape);
  });

  it("mixed portrait + landscape — no width exceeds containerWidth", () => {
    const items = [
      ...makePortrait(3).map((it, i) => ({ ...it, id: `p${i}` })),
      ...make169(3).map((it, i) => ({ ...it, id: `l${i}` })),
    ];
    const rows = buildGalleryRows(items, OPTS);
    for (const row of rows) {
      const total = row.widths.reduce((a, b) => a + b, 0) + OPTS.gap * (row.count - 1);
      expect(total).toBeLessThanOrEqual(OPTS.containerWidth + 0.01);
    }
  });

  it("single ultrawide item lands on its own row", () => {
    const ultrawide = makeItem({ id: "uw", image_w: 10000, image_h: 100 });
    const rows = buildGalleryRows([ultrawide], OPTS);
    expect(rows).toHaveLength(1);
    expect(rows[0].count).toBe(1);
  });

  it("items missing dimensions use defaultRatio", () => {
    const noDims = Array.from({ length: 5 }, (_, i) =>
      makeItem({ id: String(i) }),
    );
    const withDims = Array.from({ length: 5 }, (_, i) =>
      makeItem({ id: String(i), image_w: 1920, image_h: 1080 }),
    );
    const r1 = buildGalleryRows(noDims, OPTS);
    const r2 = buildGalleryRows(withDims, OPTS);
    // 16/9 is the default ratio and 1920/1080 = 16/9, so layouts should match
    expect(r1.length).toBe(r2.length);
    for (let i = 0; i < r1.length; i++) {
      expect(r1[i].count).toBe(r2[i].count);
      expect(r1[i].height).toBeCloseTo(r2[i].height, 1);
    }
  });

  it("last partial row height === targetRowHeight (not stretched)", () => {
    const items = make169(3); // likely a partial last row
    const rows = buildGalleryRows(items, OPTS);
    const last = rows[rows.length - 1];
    expect(last.height).toBeLessThanOrEqual(OPTS.targetRowHeight + 0.01);
  });

  it("minRowScale enforcement — row does not squash below floor", () => {
    // Many narrow portrait items: adding enough should eventually force a row break
    const narrowItems = Array.from({ length: 30 }, (_, i) =>
      makeItem({ id: String(i), image_w: 100, image_h: 1000 }),
    );
    const rows = buildGalleryRows(narrowItems, OPTS);
    for (const row of rows.slice(0, -1)) {
      expect(row.height).toBeGreaterThanOrEqual(
        OPTS.targetRowHeight * OPTS.minRowScale - 0.01,
      );
    }
  });

  it("startIndex and count are consistent across all rows", () => {
    const items = make169(15);
    const rows = buildGalleryRows(items, OPTS);
    let expected = 0;
    for (const row of rows) {
      expect(row.startIndex).toBe(expected);
      expected += row.count;
    }
    expect(expected).toBe(items.length);
  });
});
