import { createContext } from 'preact';
import { useCallback, useContext, useEffect, useState } from 'preact/hooks';
import type { ComponentChildren } from 'preact';
import { APIClient } from './client';
import { Session } from './session';
import { Device, User, WatcherPartner, WatchingPartner } from './api';

const APIContext = createContext<APIClient | null>(null);
const SetClientContext = createContext<(client: APIClient | null) => void>(() => {});

export function useAPIContext(): APIClient | null {
  return useContext(APIContext);
}

export function useSetAPIClient(): (client: APIClient | null) => void {
  return useContext(SetClientContext);
}

export function APIProvider({ children }: { children: ComponentChildren }) {
  const [client, setClient] = useState<APIClient | null>(null);
  const [ready, setReady] = useState(false);

  const updateClient = useCallback((next: APIClient | null) => {
    setClient((prev) => {
      if (prev === next) return prev;
      return next;
    });
  }, []);

  useEffect(() => {
    let cancelled = false;
    Session.restore()
      .then((session) => {
        if (cancelled) return;
        if (session) {
          updateClient(new APIClient(session));
        }
      })
      .catch((err) => console.warn('[APIProvider] session restore failed', err))
      .finally(() => {
        if (!cancelled) setReady(true);
      });
    return () => {
      cancelled = true;
    };
  }, [updateClient]);

  useEffect(() => {
    if (!client) return;
    const off = client.onLogout(() => {
      setClient(null);
    });
    return off;
  }, [client]);

  if (!ready) {
    return <div class="splash">Loading…</div>;
  }

  return (
    <APIContext.Provider value={client}>
      <SetClientContext.Provider value={updateClient}>{children}</SetClientContext.Provider>
    </APIContext.Provider>
  );
}

export function useUser(): User | null {
  const api = useAPIContext();
  const [user, setUser] = useState<User | null>(() => api?.getUser() ?? null);

  useEffect(() => {
    if (!api) {
      setUser(null);
      return;
    }
    const { user: initial, unsubscribe } = api.subscribeUser(setUser);
    setUser(initial);
    return unsubscribe;
  }, [api]);

  return user;
}

export function usePartners(): {
  watchers: WatcherPartner[];
  watchings: WatchingPartner[];
  loaded: boolean;
} {
  const api = useAPIContext();
  const [watchers, setWatchers] = useState<WatcherPartner[]>(() => api?.listWatchers() ?? []);
  const [watchings, setWatchings] = useState<WatchingPartner[]>(() => api?.listWatchings() ?? []);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    if (!api) {
      setWatchers([]);
      setWatchings([]);
      setLoaded(false);
      return;
    }
    let watchersLoaded = false;
    let watchingsLoaded = false;
    const maybeSetLoaded = () => {
      if (watchersLoaded && watchingsLoaded) setLoaded(true);
    };
    const subWatchers = api.subscribeWatchers((w) => {
      setWatchers(w);
      watchersLoaded = true;
      maybeSetLoaded();
    });
    const subWatchings = api.subscribeWatchings((w) => {
      setWatchings(w);
      watchingsLoaded = true;
      maybeSetLoaded();
    });
    setWatchers(subWatchers.watchers);
    setWatchings(subWatchings.watchings);
    watchersLoaded = subWatchers.loaded;
    watchingsLoaded = subWatchings.loaded;
    maybeSetLoaded();
    return () => {
      subWatchers.unsubscribe();
      subWatchings.unsubscribe();
    };
  }, [api]);

  return { watchers, watchings, loaded };
}

export function useDevices(): { devices: Device[]; loaded: boolean } {
  const api = useAPIContext();
  const [devices, setDevices] = useState<Device[]>(() => api?.listDevices() ?? []);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    if (!api) {
      setDevices([]);
      setLoaded(false);
      return;
    }
    const {
      devices: initial,
      loaded: initialLoaded,
      unsubscribe,
    } = api.subscribeDevices((d) => {
      setDevices(d);
      setLoaded(true);
    });
    setDevices(initial);
    if (initialLoaded) setLoaded(true);
    return unsubscribe;
  }, [api]);

  return { devices, loaded };
}
