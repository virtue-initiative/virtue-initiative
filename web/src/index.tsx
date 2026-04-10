import {
  LocationProvider,
  Router,
  Route,
  hydrate,
  prerender as ssr,
} from "preact-iso";
import { useEffect, useRef, useState } from "preact/hooks";

import { api } from "./api";
import { AuthProvider, useAuth } from "./context/auth";
import { E2EEProvider } from "./context/e2ee";
import { Header } from "./components/Header";
import { Home } from "./pages/Home/index";
import { Logs } from "./pages/Logs/index";
import { Auth } from "./pages/Auth/index";
import { Settings } from "./pages/Settings/index";
import { NotFound } from "./pages/_404";
import { GLOBAL_ALERT_EVENT, PARTNERS_CHANGED_EVENT } from "./events";
import "./style.css";

const GLOBAL_MESSAGE_KEY = "virtue_global_link_message";

type GlobalAlert = {
  id: string;
  message: string;
  isError: boolean;
  closing: boolean;
};

function GlobalEmailActionBanner() {
  const { token } = useAuth();
  const [alerts, setAlerts] = useState<GlobalAlert[]>([]);
  const timeoutsRef = useRef<number[]>([]);
  const loadedVerificationNoticeRef = useRef<string | null>(null);

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
  }, []);

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
    const token = params.get("verify_email_token");
    if (!token) return;

    api
      .verifyEmail(token)
      .then(() => {
        pushAlert("Email verified successfully.", false);
      })
      .catch((err: unknown) => {
        pushAlert(
          err instanceof Error ? err.message : "Failed to verify email",
          true,
        );
      })
      .finally(() => {
        const nextUrl = new URL(window.location.href);
        nextUrl.searchParams.delete("verify_email_token");
        window.history.replaceState({}, "", nextUrl.toString());
      });
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

    api
      .acceptPartnerInvite(token, inviteToken)
      .then(() => {
        pushAlert("Partner invite accepted.", false);
        clearInviteToken();
        window.dispatchEvent(new CustomEvent(PARTNERS_CHANGED_EVENT));
      })
      .catch((err: unknown) => {
        pushAlert(
          err instanceof Error ? err.message : "Failed to accept invite",
          true,
        );
        clearInviteToken();
      });
  }, [token]);

  useEffect(() => {
    if (typeof window === "undefined" || !token) {
      loadedVerificationNoticeRef.current = null;
      return;
    }

    const params = new URLSearchParams(window.location.search);
    if (params.get("verify_email_token")) {
      return;
    }

    if (loadedVerificationNoticeRef.current === token) {
      return;
    }

    let cancelled = false;

    api
      .getUser(token)
      .then((user) => {
        if (cancelled || user.email_verified) {
          return;
        }

        loadedVerificationNoticeRef.current = token;
        pushAlert(
          "Your email is not verified. Check Settings to resend the verification email.",
          true,
        );
      })
      .catch(() => {});

    return () => {
      cancelled = true;
    };
  }, [token]);

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

function AppShell() {
  const { token, ready } = useAuth();

  if (!ready) {
    return <div class="splash">Loading…</div>;
  }

  if (!token) {
    return (
      <>
        <GlobalEmailActionBanner />
        <Auth />
      </>
    );
  }

  return (
    <LocationProvider>
      <div class="app-shell">
        <Header />
        <main class="app-main">
          <GlobalEmailActionBanner />
          <Router>
            <Route path="/" component={Home} />
            <Route path="/logs" component={Logs} />
            <Route path="/logs/gallery" component={Logs} />
            <Route path="/settings" component={Settings} />
            <Route default component={NotFound} />
          </Router>
        </main>
      </div>
    </LocationProvider>
  );
}

export function App() {
  return (
    <AuthProvider>
      <E2EEProvider>
        <AppShell />
      </E2EEProvider>
    </AuthProvider>
  );
}

if (typeof window !== "undefined") {
  hydrate(<App />, document.getElementById("app"));
}

export async function prerender(data) {
  return await ssr(<App {...data} />);
}
