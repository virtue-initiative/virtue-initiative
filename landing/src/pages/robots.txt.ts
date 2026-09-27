import type { APIRoute } from 'astro';

// Production allows everything and points crawlers at the sitemap. Other modes
// (staging) still allow crawling so bots can read the per-page noindex tag from
// NoindexMeta.astro; a Disallow here would hide that tag and leave URLs that
// were linked from elsewhere indexable without content.
export const GET: APIRoute = ({ site }) => {
  const lines = ['User-agent: *', 'Allow: /'];
  if (import.meta.env.MODE === 'production') {
    lines.push('', `Sitemap: ${new URL('/sitemap-index.xml', site)}`);
  }
  return new Response(`${lines.join('\n')}\n`, {
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  });
};
