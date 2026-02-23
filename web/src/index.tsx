import { LocationProvider, Router, Route, hydrate, prerender as ssr } from 'preact-iso';

import { AuthProvider, useAuth } from './context/auth';
import { Header } from './components/Header';
import { Home } from './pages/Home/index';
import { Auth } from './pages/Auth/index';
import { NotFound } from './pages/_404';
import './style.css';

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
      <Header />
      <main>
        <Router>
          <Route path="/" component={Home} />
          <Route default component={NotFound} />
        </Router>
      </main>
    </LocationProvider>
  );
}

export function App() {
  return (
    <AuthProvider>
      <AppShell />
    </AuthProvider>
  );
}

if (typeof window !== 'undefined') {
  hydrate(<App />, document.getElementById('app'));
}

export async function prerender(data) {
  return await ssr(<App {...data} />);
}

