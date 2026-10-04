import { readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Keeps the staging deployment out of search engines entirely. Runs after
 * `astro build --mode staging`, before `wrangler deploy --env staging`.
 *
 * - Writes a `_headers` file so Cloudflare sends `X-Robots-Tag: noindex` on
 *   every staging response. That covers images, the appcast, and other
 *   non-HTML files the NoindexMeta tag can't reach.
 * - Deletes the generated sitemaps, so staging never advertises URLs.
 *
 * robots.txt on staging deliberately stays `Allow: /`. A `Disallow` would stop
 * crawlers from fetching pages and seeing the noindex, and a disallowed URL
 * that is linked from elsewhere can still be indexed without its content.
 */
const distDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../dist');

await writeFile(path.join(distDir, '_headers'), '/*\n  X-Robots-Tag: noindex, nofollow\n');

for (const name of await readdir(distDir)) {
  if (/^sitemap.*\.xml$/.test(name)) {
    await rm(path.join(distDir, name));
  }
}
