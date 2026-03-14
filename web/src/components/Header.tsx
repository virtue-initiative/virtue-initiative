import { useLocation } from "preact-iso";
import { useAuth } from "../context/auth";
import { ThemeButton } from "./ThemeButton";

export function Header() {
  const { token, ready, logout } = useAuth();
  const { path: currentPath } = useLocation();

  if (!ready || !token) return null;

  const isActive = (routePath: string) =>
    routePath === "/" ? currentPath === "/" : currentPath.startsWith(routePath);

  return (
    <header class="site-header">
      <div class="header-brand-row">
        <a href="/" class="header-brand">
          The Virtue Initiative
        </a>
      </div>
      <nav class="header-toolbar">
        <div class="header-nav-group">
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
        </div>

        <div class="header-action-group">
          <ThemeButton />

          <button class="control" onClick={logout} type="button">
            Log out
          </button>
        </div>
      </nav>
    </header>
  );
}
