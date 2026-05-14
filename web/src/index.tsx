import * as preact from "preact";
import {
  LocationProvider,
  Router,
  Route,
  hydrate,
  prerender as ssr,
  useLocation,
} from "preact-iso";
import { useEffect, useState } from "preact/hooks";
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
import { appSWRConfig } from "./swr";
import { ToastProvider, useToast } from "@virtueinitiative/shared-web";
import { initToast } from "./utils/toast";
import "./style.css";

// Dev-only: component preview page. The dynamic import keeps it out of the production bundle.
function DevComponentsPage() {
  const [Comp, setComp] = useState<null | (() => preact.JSX.Element)>(null);
  useEffect(() => {
    import("./pages/Dev/Components/index").then((m) => {
      setComp(() => m.ComponentsPage as () => preact.JSX.Element);
    });
  }, []);
  if (!Comp) return <div class="splash">Loading…</div>;
  return <Comp />;
}

const GLOBAL_MESSAGE_KEY = "virtue_global_link_message";

function navigate(path: string, replace = false) {
  if (typeof window === "undefined") {
    return;
  }

  const method = replace ? "replaceState" : "pushState";
  window.history[method]({}, "", path);
  window.dispatchEvent(new PopStateEvent("popstate"));
}

function GlobalEmailActionHandler() {
  const { token } = useAuth();
  const { acceptPartnerInvite } = usePartners();
  const { path: currentPath } = useLocation();
  const { push } = useToast();

  useEffect(() => {
    if (typeof window === "undefined") return;
    const stored = window.sessionStorage.getItem(GLOBAL_MESSAGE_KEY);
    if (!stored) return;
    window.sessionStorage.removeItem(GLOBAL_MESSAGE_KEY);
    try {
      const parsed = JSON.parse(stored) as
        | { message: string; isError: boolean }
        | Array<{ message: string; isError: boolean }>;
      const parsedList = Array.isArray(parsed) ? parsed : [parsed];
      for (const item of parsedList.filter(
        (i) => typeof i.message === "string" && i.message.trim(),
      )) {
        push(item.message, item.isError ? "error" : "success");
      }
    } catch {
      window.sessionStorage.removeItem(GLOBAL_MESSAGE_KEY);
    }
  }, [currentPath, push]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const params = new URLSearchParams(window.location.search);
    const inviteToken = params.get("partner_invite_token");
    if (!inviteToken) return;

    if (!token) {
      return;
    }

    const nextUrl = new URL(window.location.href);
    nextUrl.searchParams.delete("partner_invite_token");
    window.history.replaceState({}, "", nextUrl.toString());

    acceptPartnerInvite(inviteToken)
      .then(() => {
        push("Partner invite accepted.", "success");
      })
      .catch((err: unknown) => {
        push(
          err instanceof Error ? err.message : "Failed to accept invite",
          "error",
        );
      });
  }, [acceptPartnerInvite, token, push]);

  return null;
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
          <GlobalEmailActionHandler />
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
            <GlobalEmailActionHandler />
            <Router>
              <Route path="/login" component={RedirectToDashboard} />
              <Route path="/signup" component={RedirectToDashboard} />
              <Route path="/forgot-password" component={RedirectToDashboard} />
              <Route path="/" component={Home} />
              <Route path="/logs" component={Logs} />
              <Route path="/logs/gallery" component={Logs} />
              <Route path="/settings" component={Settings} />
              <Route path="/verify-email" component={VerifyEmail} />
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
      <AuthProvider>
        <SWRConfig value={appSWRConfig}>
          <E2EEProvider>
            <AppShell />
          </E2EEProvider>
        </SWRConfig>
      </AuthProvider>
    </ToastProvider>
  );
}

if (typeof window !== "undefined") {
  hydrate(<App />, document.getElementById("app"));
}

export async function prerender(data) {
  return await ssr(<App {...data} />);
}
