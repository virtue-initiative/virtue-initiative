export type HelpNavItem = {
  label: string;
  href?: string;
  items?: HelpNavItem[];
};

export const helpSidebar: HelpNavItem[] = [
  {
    label: "Getting Started",
    href: "/help/getting-started",
  },
  {
    label: "Installation",
    href: "/help/installation",
    items: [
      { label: "Windows", href: "/help/installation/windows" },
      { label: "Mac", href: "/help/installation/mac" },
      { label: "Linux", href: "/help/installation/linux" },
      { label: "Android", href: "/help/installation/android" },
      { label: "iOS", href: "/help/installation/ios" },
    ],
  },
  {
    label: "Removing Access",
    href: "/help/removing-access",
    items: [
      { label: "Whitelisting", href: "/help/removing-access/whitelisting" },
      { label: "Filtering", href: "/help/removing-access/filtering" },
      {
        label: "Disable The Browser",
        href: "/help/removing-access/disable-browser",
      },
    ],
  },
  {
    label: "Tips",
    href: "/help/tips",
  },
  {
    label: "Web",
    href: "/help/web",
    items: [
      { label: "Inviting a Partner", href: "/help/web/inviting-a-partner" },
    ],
  },
];

function flatten(items: HelpNavItem[]): HelpNavItem[] {
  return items.flatMap((item) => [
    item,
    ...(item.items ? flatten(item.items) : []),
  ]);
}

export const flatHelpSidebar = flatten(helpSidebar);

export function findHelpItem(pathname: string) {
  return flatHelpSidebar.find((item) => item.href === pathname);
}
