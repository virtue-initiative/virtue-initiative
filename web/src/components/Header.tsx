import { useLocation } from 'preact-iso';
import { useAuth } from '../context/auth';
import { useDarkMode } from '../hooks/useDarkMode';

export function Header() {
  const { token, ready, logout } = useAuth();
  const { url } = useLocation();
  const { dark, toggle } = useDarkMode();

  if (!ready || !token) return null;

  const isActive = (path: string) =>
    path === '/' ? url === '/' : url.startsWith(path);

  return (
    <header>
      <a href="/" class="header-brand">The Virtue Initiative</a>
      <nav>
        <a href="/" class={isActive('/') ? 'nav-active' : ''}>Dashboard</a>
        <a href="/logs" class={isActive('/logs') ? 'nav-active' : ''}>Logs</a>
        <button class="btn-theme" onClick={toggle} type="button" aria-label="Toggle dark mode">
          {dark ? '☀' : '🌙'}
        </button>
        <button class="header-logout" onClick={logout} type="button">
          Log out
        </button>
      </nav>
    </header>
  );
}

