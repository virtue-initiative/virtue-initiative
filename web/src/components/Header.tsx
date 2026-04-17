import { useEffect, useState } from "preact/hooks";
import { useLocation } from "preact-iso";
import { useAuth } from "../context/auth";
import { ThemeButton } from "./ThemeButton";

function MenuIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      strokeWidth="1.5"
      stroke="currentColor"
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M3.75 6.75h16.5M3.75 12h16.5m-16.5 5.25h16.5"
      />
    </svg>
  );
}

function CloseIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      strokeWidth="1.5"
      stroke="currentColor"
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M6 18 18 6M6 6l12 12"
      />
    </svg>
  );
}

export function Header() {
  const { token, ready, logout } = useAuth();
  const { path: currentPath } = useLocation();
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);

  if (!ready || !token) return null;

  const isActive = (routePath: string) =>
    routePath === "/" ? currentPath === "/" : currentPath.startsWith(routePath);

  useEffect(() => {
    setMobileMenuOpen(false);
  }, [currentPath]);

  function toggleMobileMenu() {
    setMobileMenuOpen((open) => !open);
  }

  function closeMobileMenu() {
    setMobileMenuOpen(false);
  }

  return (
    <header class="app-header">
      <div class="app-header-brand-row">
        <a href="/" class="app-header-brand">
          The Virtue Initiative
        </a>
      </div>
      <div class="app-header-mobile-actions">
        <ThemeButton />
        <button
          class="control control-icon"
          type="button"
          aria-expanded={mobileMenuOpen}
          aria-controls="mobile-header-menu"
          aria-label={
            mobileMenuOpen ? "Close navigation menu" : "Open navigation menu"
          }
          onClick={toggleMobileMenu}
        >
          <MenuIcon />
        </button>
      </div>
      <nav class="app-header-toolbar">
        <div class="app-header-nav-group">
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

        <div class="app-header-action-group">
          <ThemeButton />

          <button class="control" onClick={logout} type="button">
            Log out
          </button>
        </div>
      </nav>
      <button
        class={`app-drawer-backdrop app-header-mobile-backdrop${mobileMenuOpen ? " is-open" : ""}`}
        type="button"
        aria-label="Close navigation menu"
        onClick={closeMobileMenu}
      />
      <nav
        id="mobile-header-menu"
        class={`app-header-mobile-menu${mobileMenuOpen ? " is-open" : ""}`}
      >
        <div class="app-drawer-header app-header-mobile-menu-header">
          <span class="app-header-mobile-menu-title">Menu</span>
          <button
            class="app-drawer-close app-header-mobile-menu-close"
            type="button"
            aria-label="Close navigation menu"
            onClick={closeMobileMenu}
          >
            <CloseIcon />
          </button>
        </div>
        <a
          href="/"
          class={`control control-flat ${isActive("/") ? "is-active" : ""}`}
          onClick={closeMobileMenu}
        >
          Dashboard
        </a>
        <a
          href="/logs"
          class={`control control-flat ${isActive("/logs") ? "is-active" : ""}`}
          onClick={closeMobileMenu}
        >
          Logs
        </a>
        <a
          href="/settings"
          class={`control control-flat ${isActive("/settings") ? "is-active" : ""}`}
          onClick={closeMobileMenu}
        >
          Settings
        </a>
        <button
          class="control app-header-mobile-logout"
          onClick={() => {
            closeMobileMenu();
            logout();
          }}
          type="button"
        >
          Log out
        </button>
      </nav>
    </header>
  );
}
