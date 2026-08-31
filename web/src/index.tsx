import * as preact from 'preact';
import {
  LocationProvider,
  Router,
  Route,
  hydrate,
  prerender as ssr,
  useLocation,
} from 'preact-iso';
import { useEffect, useState } from 'preact/hooks';

import { APIProvider, useAPIContext } from './utils/api';
import { Sidebar } from './components/Sidebar';
import { Devices } from './pages/Devices/index';
import { Partners } from './pages/Partners/index';
import { Logs } from './pages/Logs/index';
import { Auth } from './pages/Auth/index';
import { InviteAccept } from './pages/InviteAccept/index';
import { Settings } from './pages/Settings/index';
import { VerifyEmail } from './pages/VerifyEmail/index';
import { NotFound } from './pages/_404';
import { ToastProvider, useToast } from '@virtueinitiative/shared-web';
import { initToast } from './utils/toast';
import './style.css';

// Dev-only: component preview page. The dynamic import keeps it out of the production bundle.
function DevComponentsPage() {
  const [Comp, setComp] = useState<null | (() => preact.JSX.Element)>(null);
  useEffect(() => {
    import('./pages/Dev/Components/index').then((m) => {
      setComp(() => m.ComponentsPage as () => preact.JSX.Element);
    });
  }, []);
  if (!Comp) return <div class="splash">Loading…</div>;
  return <Comp />;
}

const GLOBAL_MESSAGE_KEY = 'virtue_global_link_message';

function navigate(path: string, replace = false) {
  if (typeof window === 'undefined') {
    return;
  }

  const method = replace ? 'replaceState' : 'pushState';
  window.history[method]({}, '', path);
  window.dispatchEvent(new PopStateEvent('popstate'));
}

function GlobalEmailActionHandler() {
  const { path: currentPath } = useLocation();
  const { push } = useToast();

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const stored = window.sessionStorage.getItem(GLOBAL_MESSAGE_KEY);
    if (!stored) return;
    window.sessionStorage.removeItem(GLOBAL_MESSAGE_KEY);
    try {
      const parsed = JSON.parse(stored) as
        | { message: string; isError: boolean }
        | Array<{ message: string; isError: boolean }>;
      const parsedList = Array.isArray(parsed) ? parsed : [parsed];
      for (const item of parsedList.filter(
        (i) => typeof i.message === 'string' && i.message.trim(),
      )) {
        push(item.message, item.isError ? 'error' : 'success');
      }
    } catch {
      window.sessionStorage.removeItem(GLOBAL_MESSAGE_KEY);
    }
  }, [currentPath, push]);

  return null;
}

function RedirectToLogin() {
  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }

    const fullPath = window.location.pathname + window.location.search;
    const target = `/login?to=${encodeURIComponent(fullPath)}`;
    if (window.location.pathname !== '/login') {
      navigate(target, true);
    }
  }, []);

  return <div class="splash">Loading…</div>;
}

function RedirectToDashboard() {
  useEffect(() => {
    navigate('/', true);
  }, []);

  return <div class="splash">Loading…</div>;
}

function AppShell() {
  const api = useAPIContext();
  const authenticated = api !== null;

  return (
    <LocationProvider>
      {!authenticated && (
        <>
          <GlobalEmailActionHandler />
          <Router>
            <Route path="/login" component={() => <Auth mode="login" />} />
            <Route path="/signup" component={() => <Auth mode="signup" />} />
            <Route path="/forgot-password" component={() => <Auth mode="forgot-password" />} />
            <Route path="/verify-email" component={VerifyEmail} />
            <Route default component={RedirectToLogin} />
          </Router>
        </>
      )}

      {authenticated && (
        <div class="app-shell">
          <Sidebar />
          <main class="app-main">
            <GlobalEmailActionHandler />
            <Router>
              <Route path="/login" component={RedirectToDashboard} />
              <Route path="/signup" component={RedirectToDashboard} />
              <Route path="/forgot-password" component={RedirectToDashboard} />
              <Route path="/" component={Devices} />
              <Route path="/partners" component={Partners} />
              <Route path="/logs" component={Logs} />
              <Route path="/logs/list" component={Logs} />
              <Route path="/logs/gallery" component={Logs} />
              <Route path="/logs/:userId" component={Logs} />
              <Route path="/logs/:userId/list" component={Logs} />
              <Route path="/logs/:userId/gallery" component={Logs} />
              <Route path="/settings" component={Settings} />
              <Route path="/verify-email" component={VerifyEmail} />
              <Route path="/invite-accept" component={InviteAccept} />
              {import.meta.env.DEV && (
                <Route path="/dev/components" component={DevComponentsPage} />
              )}
              <Route default component={NotFound} />
            </Router>
          </main>
        </div>
      )}
    </LocationProvider>
  );
}

function ToastBridge() {
  const { push } = useToast();
  initToast(push);
  return null;
}

export function App() {
  return (
    <ToastProvider>
      <ToastBridge />
      <APIProvider>
        <AppShell />
      </APIProvider>
    </ToastProvider>
  );
}

if (typeof window !== 'undefined') {
  hydrate(<App />, document.getElementById('app'));
}

export async function prerender(data) {
  return await ssr(<App {...data} />);
}
