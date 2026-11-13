import { useCallback, useRef, useState } from 'preact/hooks';

/**
 * Runs an async function at most once at a time. A call made while a previous
 * one is still pending is ignored and resolves to `undefined`. The guard is a
 * ref, so it holds for two clicks in the same tick, before any re-render could
 * disable the button (WEB-020, WEB-021).
 */
export function useSingleFlight() {
  const pending = useRef(false);
  return useCallback(async <T>(run: () => Promise<T>): Promise<T | undefined> => {
    if (pending.current) return undefined;
    pending.current = true;
    try {
      return await run();
    } finally {
      pending.current = false;
    }
  }, []);
}

/**
 * Tracks whether a promise is pending. Pass a function (`run(() => api.x())`)
 * to also ignore a second run while one is pending, before the request starts.
 * Passing an already started promise only tracks it.
 */
export function usePromise(): [
  boolean,
  (promise: Promise<unknown> | (() => Promise<unknown>)) => void,
] {
  const [waiting, setWaiting] = useState(false);
  const pending = useRef(false);
  const setPromise = (source: Promise<unknown> | (() => Promise<unknown>)) => {
    if (typeof source === 'function') {
      if (pending.current) return;
      pending.current = true;
      setWaiting(true);
      source().finally(() => {
        pending.current = false;
        setWaiting(false);
      });
      return;
    }
    setWaiting(true);
    source.finally(() => setWaiting(false));
  };
  return [waiting, setPromise];
}
