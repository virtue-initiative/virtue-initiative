import { useState } from "preact/hooks";

export function useUrlState<T>(
  key: string,
  type: "string" | "number" | "boolean" | "object",
  defaultValue: T,
): [T, (value: T) => void] {
  const [state, setState] = useState<T>(() => {
    const urlParams = new URLSearchParams(window.location.search);
    const storedValue = urlParams.get(key);
    if (storedValue === null) {
      return defaultValue;
    }

    if (type === "string") {
      return storedValue as unknown as T;
    } else if (type === "number") {
      const num = Number(storedValue);
      return isNaN(num) ? defaultValue : (num as unknown as T);
    } else if (type === "boolean") {
      return (storedValue === "true") as unknown as T;
    } else if (type === "object") {
      try {
        return JSON.parse(storedValue) as T;
      } catch {
        return defaultValue;
      }
    }
  });

  const setUrlState = (value: T) => {
    setState(value);
    const urlParams = new URLSearchParams(window.location.search);

    if (type === "string") {
      urlParams.set(key, String(value));
    } else if (type === "number") {
      urlParams.set(key, String(value));
    } else if (type === "boolean") {
      urlParams.set(key, value ? "true" : "false");
    } else if (type === "object") {
      urlParams.set(key, JSON.stringify(value));
    }

    if (value === defaultValue) {
      urlParams.delete(key);
    }

    window.history.replaceState(
      {},
      "",
      `${window.location.pathname}?${urlParams.toString()}`,
    );
  };

  return [state, setUrlState];
}
