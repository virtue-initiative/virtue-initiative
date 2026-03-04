import {
  LocationProvider,
  Router,
  Route,
  hydrate,
  prerender as ssr,
} from "preact-iso";

import { AuthProvider, useAuth } from "./context/auth";
import { E2EEProvider } from "./context/e2ee";
import { Header } from "./components/Header";
import { Home } from "./pages/Home/index";
import { Logs } from "./pages/Logs/index";
import { Auth } from "./pages/Auth/index";
import { Settings } from "./pages/Settings/index";
import { NotFound } from "./pages/_404";
import "./style.css";

function AppShell() {
  const { token, ready } = useAuth();

  if (!ready) {
    return <div class="splash">Loading…</div>;
  }

  if (!token) {
    return <Auth />;
  }

  return (
    <LocationProvider>
      <div class="app-shell">
        <Header />
        <main class="app-main">
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
