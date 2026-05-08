import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { Batch } from "../api";
import { decryptAndFlattenBatch } from "../batch-materializer";
import {
  getUnmaterializedBatches,
  pruneDecryptedEventsBefore,
  writeMaterializedEvents,
} from "../data-cache";

const DECRYPT_CONCURRENCY = 5;
const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;

export interface DecryptedEventSyncState {
  synced: number;
  pending: number;
  total: number;
}

export function useDecryptedEventSync({
  viewerId,
  batches,
  unwrapEncryptedBatchKey,
  privateKeyReady,
}: {
  viewerId: string | null;
  batches: Batch[];
  unwrapEncryptedBatchKey: (encryptedKey: string) => Promise<CryptoKey>;
  privateKeyReady: boolean;
}): DecryptedEventSyncState {
  const [state, setState] = useState<DecryptedEventSyncState>({
    synced: 0,
    pending: 0,
    total: 0,
  });

  // Stable key so the effect only re-fires when the batch set actually changes
  const batchesKey = useMemo(
    () => batches.map((b) => b.id).join(","),
    [batches],
  );

  // Keep unwrapEncryptedBatchKey stable across renders via ref
  const unwrapRef = useRef(unwrapEncryptedBatchKey);
  unwrapRef.current = unwrapEncryptedBatchKey;

  useEffect(() => {
    if (!viewerId || !privateKeyReady || batches.length === 0) return;

    let cancelled = false;

    async function run() {
      const cutoff = Date.now() - THIRTY_DAYS_MS;

      try {
        await pruneDecryptedEventsBefore(viewerId!, cutoff);
      } catch (err) {
        console.warn("[sync] failed to prune old events", err);
      }

      let pending: Batch[];
      try {
        pending = await getUnmaterializedBatches(viewerId!, batches, cutoff);
      } catch (err) {
        console.warn("[sync] failed to get unmaterialized batches", err);
        return;
      }

      if (cancelled) return;

      setState({ synced: 0, pending: pending.length, total: batches.length });

      if (pending.length === 0) return;

      let synced = 0;
      // Sort newest-first so recently active days fill in before old history
      const queue = [...pending].sort((a, b) => b.created_at - a.created_at);

      async function worker() {
        while (queue.length > 0 && !cancelled) {
          const batch = queue.shift()!;
          try {
            const events = await decryptAndFlattenBatch(
              batch,
              unwrapRef.current,
            );
            if (cancelled) return;
            await writeMaterializedEvents(
              viewerId!,
              batch.id,
              batch.device_id,
              batch.created_at,
              events,
            );
            if (cancelled) return;
            synced++;
            setState((prev) => ({
              ...prev,
              synced,
              pending: prev.pending - 1,
            }));
          } catch (err) {
            if (!cancelled) {
              console.warn("[sync] failed to materialize batch", batch.id, err);
            }
          }
        }
      }

      await Promise.all(
        Array.from(
          { length: Math.min(DECRYPT_CONCURRENCY, pending.length) },
          worker,
        ),
      );
    }

    void run();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [viewerId, privateKeyReady, batchesKey]);

  return state;
}
