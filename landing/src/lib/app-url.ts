const DEFAULT_APP_URL = 'http://localhost:5173';

export const APP_URL = import.meta.env.PUBLIC_APP_URL || DEFAULT_APP_URL;

// Markdown content links to the web app as `/app` and `/signup`, since the
// app's origin differs per environment. Rewrite them to the real app URL.
export function rewriteAppLinks(html: string) {
  return html
    .replaceAll('href="/app"', `href="${APP_URL}"`)
    .replaceAll('href="/signup"', `href="${APP_URL}/signup"`);
}
