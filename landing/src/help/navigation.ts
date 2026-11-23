import { z } from 'astro/zod';

// Help pages are grouped by what the reader is trying to do. The topic lives in each page's
// frontmatter, not its path, so regrouping never changes a URL: the apps link to several help
// pages directly (/help/web/*, /help/what-virtue-monitors/<platform>).
export const helpTopics = [
  {
    id: 'getting-started',
    label: 'Getting started',
    blurb: 'Create an account, install Virtue and invite a partner.',
    links: [
      {
        title: 'Install Virtue',
        href: '/download',
        description: 'Download the app for Windows, Mac, Linux, Android, iPhone or iPad.',
        order: 3,
      },
    ],
  },
  {
    id: 'partners',
    label: 'Partners',
    blurb: 'Invite partners and see what they can see.',
  },
  {
    id: 'activity',
    label: 'Reviewing activity',
    blurb: 'Log types, concern scores and device status.',
  },
  {
    id: 'monitoring',
    label: 'What Virtue monitors',
    blurb: 'What each app captures on each device.',
  },
  {
    id: 'protection',
    label: 'Staying protected',
    blurb: 'Close the gaps Virtue cannot watch.',
  },
  {
    id: 'account',
    label: 'Account and security',
    blurb: 'Passwords, email and how your data is encrypted.',
  },
  {
    id: 'troubleshooting',
    label: 'Troubleshooting',
    blurb: 'Fix common problems or report a new one.',
  },
  {
    id: 'encouragement',
    label: 'Encouragement',
    blurb: 'Tips for fighting pornography and further reading.',
  },
] as const satisfies readonly HelpTopicDefinition[];

type HelpTopicDefinition = {
  id: string;
  label: string;
  blurb: string;
  // Pages outside /help that belong in the topic, such as the download page.
  links?: { title: string; href: string; description: string; order: number }[];
};

export type HelpTopicId = (typeof helpTopics)[number]['id'];

const topicIds = helpTopics.map((topic) => topic.id) as [HelpTopicId, ...HelpTopicId[]];

const frontmatterSchema = z.object({
  title: z.string(),
  // Shown on topic pages and in search results, so every page needs one.
  description: z.string(),
  // A shorter label for the sidebar, when the title is long.
  navTitle: z.string().optional(),
  topic: z.enum(topicIds),
  order: z.number(),
});

type HelpModule = {
  frontmatter: unknown;
  Content: any;
  compiledContent?: () => Promise<string>;
};

export type HelpArticle = z.infer<typeof frontmatterSchema> & {
  slug: string;
  href: string;
  module: HelpModule;
};

export type HelpLink = {
  title: string;
  navTitle?: string;
  description: string;
  href: string;
  order: number;
};

const modules = import.meta.glob<HelpModule>('../content/help/**/*.{md,mdx}', { eager: true });

export const helpArticles: HelpArticle[] = Object.entries(modules).map(([key, module]) => {
  const slug = key
    .replace('../content/help/', '')
    .replace(/\.mdx?$/, '')
    .replace(/\/index$/, '');
  const parsed = frontmatterSchema.safeParse(module.frontmatter);
  if (!parsed.success) {
    throw new Error(`Bad frontmatter in ${key}: ${parsed.error.message}`);
  }
  return { ...parsed.data, slug, href: `/help/${slug}`, module };
});

export function topicHref(id: HelpTopicId) {
  return `/help/topics/${id}`;
}

export function topicLinks(id: HelpTopicId): HelpLink[] {
  const topic = helpTopics.find((t) => t.id === id);
  const extra: HelpLink[] = topic && 'links' in topic ? [...topic.links] : [];
  return [...helpArticles.filter((article) => article.topic === id), ...extra].sort(
    (a, b) => a.order - b.order || a.title.localeCompare(b.title),
  );
}

export function findHelpArticle(pathname: string) {
  return helpArticles.find((article) => article.href === pathname);
}
