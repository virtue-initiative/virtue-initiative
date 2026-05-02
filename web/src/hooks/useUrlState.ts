import { useState } from "preact/hooks";

export function useUrlState<T>(
  key: string,
  defaultValue: T,
): [T, (value: T) => void] {
  const [state, setState] = useState<T>(() => {
    const urlParams = new URLSearchParams(window.location.search);
    const storedValue = urlParams.get(key);
    return storedValue ? JSON.parse(storedValue) : defaultValue;
  });

  const setUrlState = (value: T) => {
    setState(value);
    const urlParams = new URLSearchParams(window.location.search);
    urlParams.set(key, JSON.stringify(value));
    window.history.replaceState(
      {},
      "",
      `${window.location.pathname}?${urlParams.toString()}`,
    );
  };

  return [state, setUrlState];
}
