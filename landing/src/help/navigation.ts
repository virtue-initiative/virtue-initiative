export type HelpNavItem = {
  label: string;
  href?: string;
  items?: HelpNavItem[];
};

export const helpSidebar: HelpNavItem[] = [
  {
    label: 'Getting started',
    href: '/help/getting-started',
  },
  {
    label: 'Installation',
    href: '/help/installation',
    items: [
      { label: 'Windows', href: '/help/installation/windows' },
      { label: 'Mac', href: '/help/installation/mac' },
      { label: 'Linux', href: '/help/installation/linux' },
      { label: 'Android', href: '/help/installation/android' },
      { label: 'iOS', href: '/help/installation/ios' },
    ],
  },
  {
    label: 'How it works',
    href: '/help/how-it-works',
  },
  {
    label: 'Removing access',
    href: '/help/removing-access',
    items: [
      { label: 'Whitelisting', href: '/help/removing-access/whitelisting' },
      { label: 'Filtering', href: '/help/removing-access/filtering' },
      {
        label: 'Disable the browser',
        href: '/help/removing-access/disable-browser',
      },
    ],
  },
  {
    label: 'Tips',
    href: '/help/tips',
  },
  {
    label: 'Web',
    href: '/help/web',
    items: [
      { label: 'Inviting a partner', href: '/help/web/inviting-a-partner' },
      { label: 'Log types', href: '/help/web/log-types' },
    ],
  },
  {
    label: 'Developer',
    href: '/help/developer',
    items: [
      { label: 'Lifecycle events', href: '/help/developer/lifecycle' },
      { label: 'Testing', href: '/help/developer/testing' },
      {
        label: 'Developer overrides',
        href: '/help/developer/overrides',
      },
      {
        label: 'Security and encryption',
        href: '/help/developer/security',
      },
      {
        label: 'Tamper protection',
        href: '/help/developer/tamper-controls',
      },
    ],
  },
];

function flatten(items: HelpNavItem[]): HelpNavItem[] {
  return items.flatMap((item) => [item, ...(item.items ? flatten(item.items) : [])]);
}

const flatHelpSidebar = flatten(helpSidebar);

export function findHelpItem(pathname: string) {
  return flatHelpSidebar.find((item) => item.href === pathname);
}
