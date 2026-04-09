import { useState } from "preact/hooks";

export const usePersistedState = <T>(
  key: string,
  defaultValue: T,
): [T, (value: T) => void] => {
  const [state, setState] = useState<T>(() => {
    const storedValue = localStorage.getItem(key);
    return storedValue ? JSON.parse(storedValue) : defaultValue;
  });

  const setPersistedState = (value: T) => {
    setState(value);
    localStorage.setItem(key, JSON.stringify(value));
  };

  return [state, setPersistedState];
};
