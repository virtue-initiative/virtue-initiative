import type { APIRoute } from 'astro';

// Production allows everything and points crawlers at the sitemap. Staging
// must never be indexed. It still allows crawling here, because a Disallow
// would hide the noindex tag and header (NoindexMeta.astro and
// scripts/staging-noindex.mjs), and a disallowed URL linked from elsewhere can
// still be indexed without its content.
export const GET: APIRoute = ({ site }) => {
  const lines = ['User-agent: *', 'Allow: /'];
  if (import.meta.env.MODE === 'production') {
    lines.push('', `Sitemap: ${new URL('/sitemap-index.xml', site)}`);
  }
  return new Response(`${lines.join('\n')}\n`, {
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  });
};
