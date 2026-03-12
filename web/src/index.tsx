import {
  LocationProvider,
  Router,
  Route,
  hydrate,
  prerender as ssr,
} from "preact-iso";
import { useEffect, useState } from "preact/hooks";

import { api } from "./api";
import { AuthProvider, useAuth } from "./context/auth";
import { E2EEProvider } from "./context/e2ee";
import { Header } from "./components/Header";
import { Home } from "./pages/Home/index";
import { Logs } from "./pages/Logs/index";
import { Auth } from "./pages/Auth/index";
import { Settings } from "./pages/Settings/index";
import { NotFound } from "./pages/_404";
import "./style.css";

const GLOBAL_MESSAGE_KEY = "virtue_global_link_message";

function GlobalEmailActionBanner() {
  const { token } = useAuth();
  const [message, setMessage] = useState<string | null>(null);
  const [isError, setIsError] = useState(false);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const stored = window.sessionStorage.getItem(GLOBAL_MESSAGE_KEY);
    if (!stored) return;
    window.sessionStorage.removeItem(GLOBAL_MESSAGE_KEY);
    try {
      const parsed = JSON.parse(stored) as {
        message: string;
        isError: boolean;
      };
      setMessage(parsed.message);
      setIsError(parsed.isError);
    } catch {
      window.sessionStorage.removeItem(GLOBAL_MESSAGE_KEY);
    }
  }, []);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const params = new URLSearchParams(window.location.search);
    const token = params.get("verify_email_token");
    if (!token) return;

    api
      .verifyEmail(token)
      .then(() => {
        setMessage("Email verified successfully.");
        setIsError(false);
      })
      .catch((err: unknown) => {
        setMessage(
          err instanceof Error ? err.message : "Failed to verify email",
        );
        setIsError(true);
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
      api
        .validatePartnerInvite(inviteToken)
        .then((result) => {
          setMessage(
            `Invitation from ${result.owner.name ?? result.owner.email} ready. Sign in or create an account to accept it.`,
          );
          setIsError(false);
        })
        .catch((err: unknown) => {
          setMessage(
            err instanceof Error ? err.message : "Failed to validate invite",
          );
          setIsError(true);
          clearInviteToken();
        });
      return;
    }

    api
      .acceptPartnerInvite(token, inviteToken)
      .then(() => {
        window.sessionStorage.setItem(
          GLOBAL_MESSAGE_KEY,
          JSON.stringify({
            message: "Partner invite accepted.",
            isError: false,
          }),
        );
        const nextUrl = new URL(window.location.href);
        nextUrl.searchParams.delete("partner_invite_token");
        window.location.replace(nextUrl.toString());
      })
      .catch((err: unknown) => {
        setMessage(
          err instanceof Error ? err.message : "Failed to accept invite",
        );
        setIsError(true);
        clearInviteToken();
      });
  }, [token]);

  if (!message) {
    return null;
  }

  return (
    <p
      class={
        isError ? "alert-error global-alert" : "alert-success global-alert"
      }
    >
      {message}
    </p>
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
