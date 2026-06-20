import { useEffect, useRef, useState } from 'preact/hooks';
import type { ComponentChildren } from 'preact';
import { useLocation } from 'preact-iso';
import { useAPIContext, useDevices, usePartners, useUser } from '../utils/api';
import { Avatar } from '@virtueinitiative/shared-web';

const HELP_URL = 'https://virtueinitiative.org/help/';

type IconProps = { class?: string };

function DevicesIcon(_: IconProps) {
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
        d="M9 17.25v1.007a3 3 0 0 1-.879 2.122L7.5 21h9l-.621-.621A3 3 0 0 1 15 18.257V17.25m6-12V15a2.25 2.25 0 0 1-2.25 2.25H5.25A2.25 2.25 0 0 1 3 15V5.25m18 0A2.25 2.25 0 0 0 18.75 3H5.25A2.25 2.25 0 0 0 3 5.25m18 0V12a2.25 2.25 0 0 1-2.25 2.25H5.25A2.25 2.25 0 0 1 3 12V5.25"
      />
    </svg>
  );
}

function PartnersIcon(_: IconProps) {
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
        d="M15 19.128a9.38 9.38 0 0 0 2.625.372 9.337 9.337 0 0 0 4.121-.952 4.125 4.125 0 0 0-7.533-2.493M15 19.128v-.003c0-1.113-.285-2.16-.786-3.07M15 19.128v.106A12.318 12.318 0 0 1 8.624 21c-2.331 0-4.512-.645-6.374-1.766l-.001-.109a6.375 6.375 0 0 1 11.964-3.07M12 6.375a3.375 3.375 0 1 1-6.75 0 3.375 3.375 0 0 1 6.75 0Zm8.25 2.25a2.625 2.625 0 1 1-5.25 0 2.625 2.625 0 0 1 5.25 0Z"
      />
    </svg>
  );
}

function LogsIcon(_: IconProps) {
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
        d="M19.5 14.25v-2.625a3.375 3.375 0 0 0-3.375-3.375h-1.5A1.125 1.125 0 0 1 13.5 7.125v-1.5a3.375 3.375 0 0 0-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 0 0-9-9Z"
      />
    </svg>
  );
}

function SettingsIcon(_: IconProps) {
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
        d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.325.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 0 1 1.37.49l1.296 2.247a1.125 1.125 0 0 1-.26 1.431l-1.003.827c-.293.241-.438.613-.43.992a7.723 7.723 0 0 1 0 .255c-.008.378.137.75.43.991l1.004.827c.424.35.534.955.26 1.43l-1.298 2.247a1.125 1.125 0 0 1-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.47 6.47 0 0 1-.22.128c-.331.183-.581.495-.644.869l-.213 1.281c-.09.543-.56.94-1.11.94h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 0 1-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 0 1-1.369-.49l-1.297-2.247a1.125 1.125 0 0 1 .26-1.431l1.004-.827c.292-.24.437-.613.43-.991a6.932 6.932 0 0 1 0-.255c.007-.38-.138-.751-.43-.992l-1.004-.827a1.125 1.125 0 0 1-.26-1.43l1.297-2.247a1.125 1.125 0 0 1 1.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.086.22-.128.332-.183.582-.495.644-.869l.214-1.28Z"
      />
      <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z" />
    </svg>
  );
}

function HelpIcon(_: IconProps) {
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
        d="M9.879 7.519c1.171-1.025 3.071-1.025 4.242 0 1.172 1.025 1.172 2.687 0 3.712-.203.179-.43.326-.67.442-.745.361-1.45.999-1.45 1.827v.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9 5.25h.008v.008H12v-.008Z"
      />
    </svg>
  );
}

function LogoutIcon(_: IconProps) {
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
        d="M15.75 9V5.25A2.25 2.25 0 0 0 13.5 3h-6a2.25 2.25 0 0 0-2.25 2.25v13.5A2.25 2.25 0 0 0 7.5 21h6a2.25 2.25 0 0 0 2.25-2.25V15M12 9l-3 3m0 0 3 3m-3-3h12.75"
      />
    </svg>
  );
}

function ChevronUpIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      strokeWidth="1.5"
      stroke="currentColor"
      aria-hidden="true"
    >
      <path strokeLinecap="round" strokeLinejoin="round" d="m4.5 15.75 7.5-7.5 7.5 7.5" />
    </svg>
  );
}

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
      <path strokeLinecap="round" strokeLinejoin="round" d="M6 18 18 6M6 6l12 12" />
    </svg>
  );
}

function NavLink({
  href,
  active,
  icon,
  count,
  onNavigate,
  children,
}: {
  href: string;
  active: boolean;
  icon?: ComponentChildren;
  count?: number;
  onNavigate: () => void;
  children: ComponentChildren;
}) {
  return (
    <a
      href={href}
      class={`sidebar-nav-link${active ? ' is-active' : ''}${icon ? '' : ' sidebar-nav-sublink'}`}
      aria-current={active ? 'page' : undefined}
      onClick={onNavigate}
    >
      {icon && <span class="sidebar-nav-icon">{icon}</span>}
      <span class="sidebar-nav-label">{children}</span>
      {count != null && count > 0 && <span class="sidebar-nav-count">{count}</span>}
    </a>
  );
}

export function Sidebar() {
  const api = useAPIContext();
  const user = useUser();
  const { watchings, watchers } = usePartners();
  const { devices } = useDevices();
  const { path: currentPath } = useLocation();
  const [mobileOpen, setMobileOpen] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const footerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setMobileOpen(false);
    setDrawerOpen(false);
  }, [currentPath]);

  useEffect(() => {
    if (!drawerOpen) return;
    function close(e: MouseEvent) {
      if (footerRef.current && !footerRef.current.contains(e.target as Node)) {
        setDrawerOpen(false);
      }
    }
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [drawerOpen]);

  if (!api) return null;

  const acceptedWatchings = watchings.filter((partner) => partner.status === 'accepted');
  const onLogs = currentPath === '/logs' || currentPath.startsWith('/logs/');
  const logsUserMatch = currentPath.match(/^\/logs\/([^/]+)/);
  const activeLogsUserId = logsUserMatch ? logsUserMatch[1] : null;
  const activeUserId = activeLogsUserId === 'gallery' ? null : activeLogsUserId;

  const deviceCount = devices.filter((device) => device.owner === api.userId).length;
  const partnerCount =
    acceptedWatchings.length + watchers.filter((partner) => partner.status === 'accepted').length;

  const logout = () => {
    void api.logout();
  };

  const closeMobile = () => setMobileOpen(false);
  const closeAll = () => {
    closeMobile();
    setDrawerOpen(false);
  };

  const userLabel = user?.name ?? user?.email ?? 'Account';
  const userSub = user?.name ? user?.email : undefined;

  return (
    <>
      <div class="sidebar-mobile-bar">
        <button
          class="sidebar-mobile-toggle"
          type="button"
          aria-label="Open navigation menu"
          aria-expanded={mobileOpen}
          aria-controls="app-sidebar"
          onClick={() => setMobileOpen(true)}
        >
          <MenuIcon />
        </button>
        <a href="/" class="sidebar-mobile-brand">
          The Virtue Initiative
        </a>
      </div>

      <button
        class={`app-drawer-backdrop sidebar-backdrop${mobileOpen ? ' is-open' : ''}`}
        type="button"
        aria-label="Close navigation menu"
        onClick={closeMobile}
      />

      <aside id="app-sidebar" class={`app-sidebar${mobileOpen ? ' is-open' : ''}`}>
        <div class="sidebar-brand-row">
          <a href="/" class="sidebar-brand" onClick={closeMobile}>
            The Virtue Initiative
          </a>
          <button
            class="app-drawer-close sidebar-mobile-close"
            type="button"
            aria-label="Close navigation menu"
            onClick={closeMobile}
          >
            <CloseIcon />
          </button>
        </div>

        <nav class="sidebar-nav" aria-label="Primary">
          <NavLink
            href="/"
            active={currentPath === '/'}
            icon={<DevicesIcon />}
            count={deviceCount}
            onNavigate={closeMobile}
          >
            Devices
          </NavLink>
          <NavLink
            href="/partners"
            active={currentPath.startsWith('/partners')}
            icon={<PartnersIcon />}
            count={partnerCount}
            onNavigate={closeMobile}
          >
            Partners
          </NavLink>

          <div class="sidebar-nav-group">
            <div class="sidebar-nav-group-heading">
              <span class="sidebar-nav-icon">
                <LogsIcon />
              </span>
              <span class="sidebar-nav-label">Logs</span>
            </div>
            <div class="sidebar-nav-sublist">
              <NavLink href="/logs" active={onLogs && !activeUserId} onNavigate={closeMobile}>
                My logs
              </NavLink>
              {acceptedWatchings.map((partner) => (
                <NavLink
                  key={partner.id}
                  href={`/logs/${partner.user.id}`}
                  active={onLogs && activeUserId === partner.user.id}
                  onNavigate={closeMobile}
                >
                  {(partner.user.name ?? partner.user.email) + ' logs'}
                </NavLink>
              ))}
            </div>
          </div>
        </nav>

        <div class="sidebar-footer" ref={footerRef}>
          <div class={`sidebar-footer-drawer${drawerOpen ? ' is-open' : ''}`}>
            <div class="sidebar-footer-drawer-inner">
              <NavLink
                href="/settings"
                active={currentPath.startsWith('/settings')}
                icon={<SettingsIcon />}
                onNavigate={closeAll}
              >
                Settings
              </NavLink>
              <a
                href={HELP_URL}
                class="sidebar-nav-link"
                target="_blank"
                rel="noreferrer"
                onClick={closeAll}
              >
                <span class="sidebar-nav-icon">
                  <HelpIcon />
                </span>
                <span class="sidebar-nav-label">Help</span>
              </a>
              <button
                class="sidebar-nav-link sidebar-logout"
                type="button"
                onClick={() => {
                  closeAll();
                  logout();
                }}
              >
                <span class="sidebar-nav-icon">
                  <LogoutIcon />
                </span>
                <span class="sidebar-nav-label">Log out</span>
              </button>
            </div>
          </div>

          <button
            class="sidebar-user"
            type="button"
            onClick={() => setDrawerOpen((o) => !o)}
            aria-expanded={drawerOpen}
          >
            <Avatar name={user?.name ?? user?.email} size="md" />
            <span class="sidebar-user-text">
              <span class="sidebar-user-name">{userLabel}</span>
              {userSub && <span class="sidebar-user-sub">{userSub}</span>}
            </span>
            <span class={`sidebar-user-chevron${drawerOpen ? ' is-open' : ''}`}>
              <ChevronUpIcon />
            </span>
          </button>
        </div>
      </aside>
    </>
  );
}
