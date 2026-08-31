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
): [T, (value: T) => void] {
  const { query, route } = useLocation();
  const raw = query[key];
  const state = raw === undefined ? defaultValue : parseValue(raw, type, defaultValue);

  const setUrlState = (value: T) => {
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
    route(`${window.location.pathname}${qs ? `?${qs}` : ''}`, true);
  };

  return [state, setUrlState];
}
