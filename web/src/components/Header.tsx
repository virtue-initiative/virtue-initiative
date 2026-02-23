import { useAuth } from '../context/auth';

export function Header() {
  const { token, ready, logout } = useAuth();

  if (!ready || !token) return null;

  return (
    <header>
      <span class="header-brand">BePure</span>
      <nav>
        <a href="/">Dashboard</a>
        <a href="/logs">Logs</a>
        <button class="header-logout" onClick={logout} type="button">
          Log out
        </button>
      </nav>
    </header>
  );
}

