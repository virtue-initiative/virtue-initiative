import { useState } from 'preact/hooks';

export function usePromise(): [boolean, (promise: Promise<unknown>) => void] {
  const [waiting, setWaiting] = useState(false);
  const setPromise = (promise: Promise<unknown>) => {
    setWaiting(true);
    promise.finally(() => setWaiting(false));
  };
  return [waiting, setPromise];
}
