import { useEffect, useRef, useState } from 'preact/hooks';
import type { ComponentChildren } from 'preact';
import { useLocation } from 'preact-iso';
import { useAPIContext, useDevices, usePartners, useUser } from '../utils/api';
import { Avatar } from '@virtueinitiative/shared-web';
import { DevicesIcon, PartnersIcon, LogsIcon, SettingsIcon } from './icons';
import { LANDING_URL } from '../utils/landing-url';
import { ReportBugDialog } from './ReportBugDialog';

const HELP_URL = `${LANDING_URL}/help`;

type IconProps = { class?: string };

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

function BugIcon(_: IconProps) {
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
        d="M12 12.75a3 3 0 0 0 3-3V6a3 3 0 0 0-6 0v3.75a3 3 0 0 0 3 3Zm0 0v6.75m-6-4.5H3.75m2.25-4.5-1.5-1.5M18 15h2.25m-2.25-4.5 1.5-1.5M8.25 6 6.75 4.5m10.5 1.5L18.75 4.5M6 15a6 6 0 0 0 12 0v-1.5a6 6 0 0 0-12 0V15Z"
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
  const reportBugDialogRef = useRef<HTMLDialogElement>(null);

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
  // `/logs/list` and `/logs/gallery` are view segments, not user ids.
  const activeUserId =
    activeLogsUserId === 'gallery' || activeLogsUserId === 'list' ? null : activeLogsUserId;

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

  const userLabel = user?.name || user?.email || 'Account';

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
            <a href="/logs" class="sidebar-nav-group-heading" onClick={closeMobile}>
              <span class="sidebar-nav-icon">
                <LogsIcon />
              </span>
              <span class="sidebar-nav-label">Logs</span>
            </a>
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
                class="sidebar-nav-link"
                type="button"
                onClick={() => {
                  closeAll();
                  reportBugDialogRef.current?.showModal();
                }}
              >
                <span class="sidebar-nav-icon">
                  <BugIcon />
                </span>
                <span class="sidebar-nav-label">Report a bug</span>
              </button>
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
            </span>
            <span class={`sidebar-user-chevron${drawerOpen ? ' is-open' : ''}`}>
              <ChevronUpIcon />
            </span>
          </button>
        </div>
      </aside>
      <ReportBugDialog dialogRef={reportBugDialogRef} />
    </>
  );
}
