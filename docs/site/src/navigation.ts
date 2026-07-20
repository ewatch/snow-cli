export interface NavItem {
  label: string;
  /** Site-root-relative route, e.g. "/quick-start/". "/" is the landing page. */
  href: string;
  children?: NavItem[];
}

export interface NavGroup {
  label: string;
  items: NavItem[];
}

// Mirrors the mdbook SUMMARY.md order, grouped for the sidebar.
export const sidebar: NavGroup[] = [
  {
    label: 'Getting started',
    items: [
      { label: 'Introduction', href: '/introduction/' },
      { label: 'Installation', href: '/installation/' },
      { label: 'Quick start', href: '/quick-start/' },
      { label: 'Testing with a PDI', href: '/pdi-testing/' },
    ],
  },
  {
    label: 'Guides',
    items: [
      { label: 'Review a Flow Designer flow', href: '/guides/review-a-flow/' },
      { label: 'Explore a custom application', href: '/guides/explore-a-custom-app/' },
      { label: 'Query instance analytics', href: '/guides/instance-analytics/' },
      { label: 'Run a background script safely', href: '/guides/run-a-background-script/' },
    ],
  },
  {
    label: 'Configuration',
    items: [
      {
        label: 'Configuration and authentication',
        href: '/configuration/',
        children: [
          { label: 'OAuth authorization code with PKCE', href: '/oauth-authorization-code-pkce/' },
          { label: 'Secure read-only usage', href: '/secure-readonly-usage/' },
        ],
      },
    ],
  },
  {
    label: 'Command reference',
    items: [
      {
        label: 'Overview',
        href: '/commands/',
        children: [
          { label: 'profile', href: '/commands/profile/' },
          { label: 'auth', href: '/commands/auth/' },
          { label: 'table', href: '/commands/table/' },
          { label: 'data', href: '/commands/data/' },
          { label: 'seed', href: '/commands/seed/' },
          { label: 'scope', href: '/commands/scope/' },
          { label: 'attachment', href: '/commands/attachment/' },
          { label: 'import-set', href: '/commands/import-set/' },
          { label: 'api', href: '/commands/api/' },
          { label: 'graphql', href: '/commands/graphql/' },
          { label: 'script', href: '/commands/script/' },
          { label: 'snu', href: '/commands/snu/' },
          { label: 'codesearch', href: '/commands/codesearch/' },
          { label: 'completions', href: '/commands/completions/' },
        ],
      },
    ],
  },
];

/** Sidebar flattened to reading order, for prev/next pagination. */
export const flatNav: NavItem[] = sidebar.flatMap((group) =>
  group.items.flatMap((item) => [
    { label: item.label, href: item.href },
    ...(item.children ?? []),
  ]),
);

/** Maps a content entry id (e.g. "commands/table") to its route. */
export function routeForEntry(id: string): string {
  return `/${id}/`;
}
