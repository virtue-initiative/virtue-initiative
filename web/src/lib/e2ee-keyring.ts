import { decode, encode } from "@msgpack/msgpack";

export type E2EEKeyRange = {
  start: number;
  end: number | null;
  key: Uint8Array;
};

type KeyringPayload = {
  version: 1;
  ranges: Array<{
    start: number;
    end: number | null;
    key: Uint8Array;
  }>;
};

function toUint8Array(value: unknown): Uint8Array | null {
  if (value instanceof Uint8Array) return value;
  if (Array.isArray(value) && value.every((item) => typeof item === "number")) {
    return Uint8Array.from(value);
  }
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (ArrayBuffer.isView(value)) {
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }
  return null;
}

export function normalizeE2EEKeyRanges(ranges: E2EEKeyRange[]): E2EEKeyRange[] {
  return [...ranges]
    .filter(
      (range) =>
        Number.isFinite(range.start) &&
        range.start >= 0 &&
        (range.end === null || Number.isFinite(range.end)),
    )
    .sort((a, b) => a.start - b.start)
    .map((range) => ({
      start: range.start,
      end: range.end,
      key: Uint8Array.from(range.key),
    }));
}

export function decodeE2EEKeyRanges(blob: Uint8Array): E2EEKeyRange[] {
  if (blob.byteLength === 32) {
    return [{ start: 0, end: null, key: Uint8Array.from(blob) }];
  }

  const decoded = decode(blob) as Partial<KeyringPayload> | undefined;
  if (!decoded || decoded.version !== 1 || !Array.isArray(decoded.ranges)) {
    throw new Error("Invalid encrypted E2EE key payload");
  }

  const ranges = decoded.ranges.flatMap((range) => {
    const key = toUint8Array(range.key);
    if (!key) return [];
    if (typeof range.start !== "number") return [];
    if (range.end !== null && typeof range.end !== "number") return [];
    return [{ start: range.start, end: range.end ?? null, key }];
  });

  if (ranges.length === 0) {
    throw new Error("Encrypted E2EE key payload has no usable key ranges");
  }

  return normalizeE2EEKeyRanges(ranges);
}

export function encodeE2EEKeyRanges(ranges: E2EEKeyRange[]): Uint8Array {
  const normalized = normalizeE2EEKeyRanges(ranges);
  const payload: KeyringPayload = {
    version: 1,
    ranges: normalized.map((range) => ({
      start: range.start,
      end: range.end,
      key: Uint8Array.from(range.key),
    })),
  };
  return Uint8Array.from(encode(payload));
}

export function selectE2EEKeyRangeForTimestamp(
  ranges: E2EEKeyRange[],
  timestamp: number,
): E2EEKeyRange | null {
  const normalized = normalizeE2EEKeyRanges(ranges);
  let selected: E2EEKeyRange | null = null;

  for (const range of normalized) {
    if (timestamp < range.start) continue;
    if (range.end !== null && timestamp > range.end) continue;
    if (!selected || range.start > selected.start) {
      selected = range;
    }
  }

  return selected;
}

export function latestE2EEKeyRange(ranges: E2EEKeyRange[]): E2EEKeyRange | null {
  const normalized = normalizeE2EEKeyRanges(ranges);
  if (normalized.length === 0) return null;
  for (let i = normalized.length - 1; i >= 0; i -= 1) {
    if (normalized[i]!.end === null) {
      return normalized[i]!;
    }
  }
  return normalized[normalized.length - 1] ?? null;
}

export function rotateE2EEKeyRanges(
  currentRanges: E2EEKeyRange[],
  nextKey: Uint8Array,
  rotatedAt: number,
): E2EEKeyRange[] {
  const normalized = normalizeE2EEKeyRanges(currentRanges);
  const current = latestE2EEKeyRange(normalized);
  const nextRanges = normalized.map((range) => ({ ...range, key: Uint8Array.from(range.key) }));

  if (current && current.end === null && current.start <= rotatedAt) {
    const currentIndex = nextRanges.findIndex((range) => range.start === current.start);
    if (currentIndex >= 0) {
      nextRanges[currentIndex] = {
        ...nextRanges[currentIndex],
        end: Math.max(nextRanges[currentIndex]!.start, rotatedAt - 1),
      };
    }
  }

  nextRanges.push({
    start: rotatedAt,
    end: null,
    key: Uint8Array.from(nextKey),
  });

  return normalizeE2EEKeyRanges(nextRanges);
}
