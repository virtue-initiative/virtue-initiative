import type { DataLog } from '../../utils/api/api';
import type { BatchVerification } from '../../utils/api/crypto';

export type FeedLog = DataLog & {
  batch_status: BatchVerification;
  source: 'batch' | 'log';
  image_w?: number;
  image_h?: number;
};

export function toUint8Array(value: unknown): Uint8Array | undefined {
  if (!value) return undefined;
  if (value instanceof Uint8Array) return value;
  if (Array.isArray(value)) return new Uint8Array(value as number[]);
  if (typeof value === 'string') {
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
