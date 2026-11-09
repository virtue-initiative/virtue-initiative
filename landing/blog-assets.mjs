import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

// Astro turns a post's `./image.png` into a hashed, re-encoded file under /_astro, which is fine
// for the blog page but no good for the newsletter: it needs a stable URL to an image every mail
// client can show (Outlook can't show WebP). So every image beside a folder post's index.md is
// also published untouched at /blog/<folder>/<file>, which is where api-donate's newsletter
// script points relative image paths.
const BLOG_DIR = fileURLToPath(new URL('./src/content/blog/', import.meta.url));
const TYPES = {
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.svg': 'image/svg+xml',
};

/** [folder, file] for every image in a folder post. */
function blogImages() {
  return readdirSync(BLOG_DIR)
    .filter((folder) => statSync(join(BLOG_DIR, folder)).isDirectory())
    .flatMap((folder) =>
      readdirSync(join(BLOG_DIR, folder))
        .filter((file) => TYPES[extname(file).toLowerCase()])
        .map((file) => [folder, file]),
    );
}

/** @returns {import('astro').AstroIntegration} */
export default function blogAssets() {
  return {
    name: 'blog-assets',
    hooks: {
      // Serve them from the dev server too, so a newsletter preview against a local site works.
      'astro:server:setup': ({ server }) => {
        server.middlewares.use((req, res, next) => {
          const match = /^\/blog\/([^/]+)\/([^/?#]+)$/.exec(req.url ?? '');
          const type = match && TYPES[extname(match[2]).toLowerCase()];
          const path = match && join(BLOG_DIR, match[1], match[2]);
          if (!type || match[1].startsWith('.') || match[2].startsWith('.') || !existsSync(path)) {
            return next();
          }
          res.setHeader('Content-Type', type);
          res.end(readFileSync(path));
        });
      },
      'astro:build:done': ({ dir }) => {
        for (const [folder, file] of blogImages()) {
          const out = fileURLToPath(new URL(`blog/${folder}/`, dir));
          mkdirSync(out, { recursive: true });
          copyFileSync(join(BLOG_DIR, folder, file), join(out, file));
        }
      },
    },
  };
}
