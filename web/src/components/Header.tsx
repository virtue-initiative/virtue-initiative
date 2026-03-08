import { useLocation } from "preact-iso";
import { useAuth } from "../context/auth";
import { ThemeButton } from "./ThemeButton";

export function Header() {
  const { token, ready, logout } = useAuth();
  const { url } = useLocation();

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

        <ThemeButton />

        <button class="control" onClick={logout} type="button">
          Log out
        </button>
      </nav>
    </header>
  );
}
