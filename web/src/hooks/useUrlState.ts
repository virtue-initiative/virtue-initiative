import { useLocation } from 'preact-iso';

function parseValue<T>(
  raw: string,
  type: 'string' | 'number' | 'boolean' | 'object',
  defaultValue: T,
): T {
  if (type === 'string') {
    return raw as unknown as T;
  } else if (type === 'number') {
    const num = Number(raw);
    return isNaN(num) ? defaultValue : (num as unknown as T);
  } else if (type === 'boolean') {
    return (raw === 'true') as unknown as T;
  }
  try {
    return JSON.parse(raw) as T;
  } catch {
    return defaultValue;
  }
}

/**
 * A piece of state that lives in the URL query string.
 *
 * The value is *derived* from the router's current location rather than held in
 * component state, so anything that changes the URL — a sidebar link, the back
 * button, another `useUrlState` setter — is reflected immediately. The setter
 * navigates through preact-iso's `route()` so `LocationProvider`'s context stays
 * in sync (writing `history.replaceState` directly would leave it stale).
 */
export function useUrlState<T>(
  key: string,
  type: 'string' | 'number' | 'boolean' | 'object',
  defaultValue: T,
  /** When given, any parsed value outside this list is treated as the default (WEB-039). */
  allowed?: readonly T[],
): [T, (value: T, options?: { replace?: boolean }) => void] {
  const { query, route } = useLocation();
  const raw = query[key];
  const parsed = raw === undefined ? defaultValue : parseValue(raw, type, defaultValue);
  const state = allowed && !allowed.includes(parsed) ? defaultValue : parsed;

  // Each change pushes a history entry so Back undoes it (WEB-039). Pass
  // `replace` for automatic corrections the user didn't make, or Back would
  // return to the value that was just corrected and loop.
  const setUrlState = (value: T, options?: { replace?: boolean }) => {
    const urlParams = new URLSearchParams(window.location.search);

    if (value === defaultValue) {
      urlParams.delete(key);
    } else if (type === 'object') {
      urlParams.set(key, JSON.stringify(value));
    } else if (type === 'boolean') {
      urlParams.set(key, value ? 'true' : 'false');
    } else {
      urlParams.set(key, String(value));
    }

    const qs = urlParams.toString();
    route(`${window.location.pathname}${qs ? `?${qs}` : ''}`, options?.replace ?? false);
  };

  return [state, setUrlState];
}
