import { useLocation } from 'preact-iso';
import { useAuth } from '../context/auth';

export function Header() {
  const { token, ready, logout } = useAuth();
  const { url } = useLocation();

  if (!ready || !token) return null;

  const isActive = (path: string) =>
    path === '/' ? url === '/' : url.startsWith(path);

  return (
    <header>
      <a href="/" class="header-brand">The Virtue Initiative</a>
      <nav>
        <a href="/" class={isActive('/') ? 'nav-active' : ''}>Dashboard</a>
        <a href="/logs" class={isActive('/logs') ? 'nav-active' : ''}>Logs</a>
        <button class="header-logout" onClick={logout} type="button">
          Log out
        </button>
      </nav>
    </header>
  );
}

