import { decode } from "@msgpack/msgpack";
import { Batch } from "./api";
import { decryptBatch, decompressGzip } from "./crypto";
import { FeedLog, toUint8Array } from "./pages/Logs/shared";

export async function decryptAndFlattenBatch(
  batch: Batch,
  openBatchKey: (encryptedKey: string) => Promise<CryptoKey>,
): Promise<FeedLog[]> {
  const response = await fetch(batch.url);
  if (!response.ok) {
    throw new Error(`Fetch failed (${response.status}) for ${batch.url}`);
  }

  const raw = new Uint8Array(await response.arrayBuffer());
  if (raw.length < 13) {
    throw new Error(`Batch blob too short for AES-GCM payload: ${batch.url}`);
  }

  const batchKey = await openBatchKey(batch.encrypted_key);
  const decrypted = await decryptBatch(batchKey, raw);
  const decompressed = await decompressGzip(decrypted);
  const decoded = decode(decompressed) as unknown;
  const eventBytes = Array.isArray(decoded) ? decoded : [];

  return eventBytes.map((encodedEvent, index) => {
    const rawEvent = toUint8Array(encodedEvent);
    if (!rawEvent) {
      throw new Error(`Batch event ${index} is not a byte array`);
    }

    const event = decode(rawEvent) as Record<string, unknown>;
    const data =
      event.data && typeof event.data === "object"
        ? (event.data as Record<string, unknown>)
        : {};

    if (Array.isArray(data.image)) {
      data.image = new Uint8Array(data.image as number[]);
    }

    return {
      id: typeof event.id === "string" ? event.id : `${batch.id}:${index}`,
      device_id: batch.device_id,
      ts: typeof event.ts === "number" ? event.ts : batch.end_time,
      type: typeof event.type === "string" ? event.type : "unknown",
      data,
      created_at: batch.created_at,
      risk: typeof event.risk === "number" ? event.risk : undefined,
      batch_status: "unknown" as const,
      source: "batch" as const,
    };
  });
}
