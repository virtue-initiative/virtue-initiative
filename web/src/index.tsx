import {
  LocationProvider,
  Router,
  Route,
  hydrate,
  prerender as ssr,
  useLocation,
} from "preact-iso";
import { useEffect, useRef, useState } from "preact/hooks";
import { SWRConfig } from "swr";

import { AuthProvider, useAuth } from "./context/auth";
import { E2EEProvider } from "./context/e2ee";
import { Header } from "./components/Header";
import { usePartners } from "./hooks/usePartners";
import { Home } from "./pages/Home/index";
import { Logs } from "./pages/Logs/index";
import { Auth } from "./pages/Auth/index";
import { VerifyEmail } from "./pages/VerifyEmail/index";
import { Settings } from "./pages/Settings/index";
import { NotFound } from "./pages/_404";
import { GLOBAL_ALERT_EVENT } from "./events";
import { appSWRConfig } from "./swr";
import "./style.css";

const GLOBAL_MESSAGE_KEY = "virtue_global_link_message";

function navigate(path: string, replace = false) {
  if (typeof window === "undefined") {
    return;
  }

  const method = replace ? "replaceState" : "pushState";
  window.history[method]({}, "", path);
  window.dispatchEvent(new PopStateEvent("popstate"));
}

type GlobalAlert = {
  id: string;
  message: string;
  isError: boolean;
  closing: boolean;
};

function GlobalEmailActionBanner() {
  const { token } = useAuth();
  const { acceptPartnerInvite } = usePartners();
  const { path: currentPath } = useLocation();
  const [alerts, setAlerts] = useState<GlobalAlert[]>([]);
  const timeoutsRef = useRef<number[]>([]);

  useEffect(
    () => () => {
      for (const timeout of timeoutsRef.current) {
        window.clearTimeout(timeout);
      }
      timeoutsRef.current = [];
    },
    [],
  );

  function dismissAlert(id: string) {
    setAlerts((previous) =>
      previous.map((alert) =>
        alert.id === id ? { ...alert, closing: true } : alert,
      ),
    );
    const removalTimeout = window.setTimeout(() => {
      setAlerts((previous) => previous.filter((alert) => alert.id !== id));
    }, 220);
    timeoutsRef.current.push(removalTimeout);
  }

  function pushAlert(message: string, isError: boolean) {
    const id = `${Date.now()}-${Math.random().toString(36).slice(2)}`;
    setAlerts((previous) => [
      ...previous,
      {
        id,
        message,
        isError,
        closing: false,
      },
    ]);
    const timeout = window.setTimeout(() => {
      dismissAlert(id);
    }, 45_000);
    timeoutsRef.current.push(timeout);
  }

  useEffect(() => {
    if (typeof window === "undefined") return;
    const stored = window.sessionStorage.getItem(GLOBAL_MESSAGE_KEY);
    if (!stored) return;
    window.sessionStorage.removeItem(GLOBAL_MESSAGE_KEY);
    try {
      const parsed = JSON.parse(stored) as
        | {
            message: string;
            isError: boolean;
          }
        | Array<{
            message: string;
            isError: boolean;
          }>;
      const parsedList = Array.isArray(parsed) ? parsed : [parsed];
      const nextAlerts = parsedList
        .filter(
          (item) => typeof item.message === "string" && item.message.trim(),
        )
        .map((item) => ({
          id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
          message: item.message,
          isError: Boolean(item.isError),
          closing: false,
        }));
      if (nextAlerts.length > 0) {
        setAlerts((previous) => [...previous, ...nextAlerts]);
      }
    } catch {
      window.sessionStorage.removeItem(GLOBAL_MESSAGE_KEY);
    }
  }, [currentPath]);

  useEffect(() => {
    if (typeof window === "undefined") return;

    function handleGlobalAlert(event: Event) {
      const detail = (
        event as CustomEvent<{
          message?: string;
          isError?: boolean;
        }>
      ).detail;

      if (!detail?.message?.trim()) {
        return;
      }

      pushAlert(detail.message, Boolean(detail.isError));
    }

    window.addEventListener(GLOBAL_ALERT_EVENT, handleGlobalAlert);
    return () => {
      window.removeEventListener(GLOBAL_ALERT_EVENT, handleGlobalAlert);
    };
  }, []);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const params = new URLSearchParams(window.location.search);
    const inviteToken = params.get("partner_invite_token");
    if (!inviteToken) return;

    const clearInviteToken = () => {
      const nextUrl = new URL(window.location.href);
      nextUrl.searchParams.delete("partner_invite_token");
      window.history.replaceState({}, "", nextUrl.toString());
    };

    if (!token) {
      return;
    }

    acceptPartnerInvite(inviteToken)
      .then(() => {
        pushAlert("Partner invite accepted.", false);
        clearInviteToken();
      })
      .catch((err: unknown) => {
        pushAlert(
          err instanceof Error ? err.message : "Failed to accept invite",
          true,
        );
        clearInviteToken();
      });
  }, [acceptPartnerInvite, token]);

  if (alerts.length === 0) {
    return null;
  }

  return (
    <div
      class={`global-alert-stack${token ? " global-alert-stack--with-header" : ""}`}
      aria-live="polite"
      aria-atomic="false"
    >
      {alerts.map((alert) => (
        <div
          key={alert.id}
          role="status"
          class={`${alert.isError ? "alert-error" : "alert-success"} global-alert${alert.closing ? " global-alert--closing" : ""}`}
        >
          <span>{alert.message}</span>
          <button
            class="global-alert-close"
            type="button"
            onClick={() => dismissAlert(alert.id)}
            aria-label="Dismiss notification"
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}

function RedirectToLogin() {
  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const target = `/login${window.location.search}`;
    if (window.location.pathname !== "/login") {
      navigate(target, true);
    }
  }, []);

  return <div class="splash">Loading…</div>;
}

function RedirectToDashboard() {
  useEffect(() => {
    navigate("/", true);
  }, []);

  return <div class="splash">Loading…</div>;
}

function AppShell() {
  const { token, ready } = useAuth();

  if (!ready) {
    return <div class="splash">Loading…</div>;
  }

  return (
    <LocationProvider>
      {!token && (
        <>
          <GlobalEmailActionBanner />
          <Router>
            <Route path="/login" component={() => <Auth mode="login" />} />
            <Route path="/signup" component={() => <Auth mode="signup" />} />
            <Route
              path="/forgot-password"
              component={() => <Auth mode="forgot-password" />}
            />
            <Route path="/verify-email" component={VerifyEmail} />
            <Route default component={RedirectToLogin} />
          </Router>
        </>
      )}

      {token && (
        <div class="app-shell">
          <Header />
          <main class="app-main">
            <GlobalEmailActionBanner />
            <Router>
              <Route path="/login" component={RedirectToDashboard} />
              <Route path="/signup" component={RedirectToDashboard} />
              <Route path="/forgot-password" component={RedirectToDashboard} />
              <Route path="/" component={Home} />
              <Route path="/logs" component={Logs} />
              <Route path="/logs/gallery" component={Logs} />
              <Route path="/settings" component={Settings} />
              <Route path="/verify-email" component={VerifyEmail} />
              <Route default component={NotFound} />
            </Router>
          </main>
        </div>
      )}
    </LocationProvider>
  );
}

export function App() {
  return (
    <AuthProvider>
      <SWRConfig value={appSWRConfig}>
        <E2EEProvider>
          <AppShell />
        </E2EEProvider>
      </SWRConfig>
    </AuthProvider>
  );
}

if (typeof window !== "undefined") {
  hydrate(<App />, document.getElementById("app"));
}

export async function prerender(data) {
  return await ssr(<App {...data} />);
}
