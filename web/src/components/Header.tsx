import { useLocation } from "preact-iso";
import { useAuth } from "../context/auth";
import { useDarkMode } from "../hooks/useDarkMode";

function MoonIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M21.752 15.002A9.718 9.718 0 0 1 18 15.75 9.75 9.75 0 0 1 8.25 6c0-1.33.266-2.596.748-3.752a9.75 9.75 0 1 0 12.754 12.754Z" />
    </svg>
  );
}

function SunIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M12 3v2.25M12 18.75V21M5.636 5.636l1.591 1.591M16.773 16.773l1.591 1.591M3 12h2.25M18.75 12H21M5.636 18.364l1.591-1.591M16.773 7.227l1.591-1.591M15.75 12a3.75 3.75 0 1 1-7.5 0 3.75 3.75 0 0 1 7.5 0Z" />
    </svg>
  );
}

export function Header() {
  const { token, ready, logout } = useAuth();
  const { url } = useLocation();
  const { dark, toggle } = useDarkMode();

  if (!ready || !token) return null;

  const isActive = (path: string) =>
    path === "/" ? url === "/" : url.startsWith(path);

  return (
    <header class="site-header">
      <a href="/" class="header-brand">
        The Virtue Initiative
      </a>
      <nav class="header-toolbar">
        <a
          href="/"
          class={`control control-flat ${isActive("/") ? "is-active" : ""}`}
        >
          Dashboard
        </a>
        <a
          href="/logs"
          class={`control control-flat ${isActive("/logs") ? "is-active" : ""}`}
        >
          Logs
        </a>
        <a
          href="/settings"
          class={`control control-flat ${isActive("/settings") ? "is-active" : ""}`}
        >
          Settings
        </a>
        <button
          class="control control-icon"
          onClick={toggle}
          type="button"
          aria-label={dark ? "Switch to light mode" : "Switch to dark mode"}
          title={dark ? "Switch to light mode" : "Switch to dark mode"}
        >
          {dark ? <SunIcon /> : <MoonIcon />}
        </button>
        <button class="control" onClick={logout} type="button">
          Log out
        </button>
      </nav>
    </header>
  );
}
