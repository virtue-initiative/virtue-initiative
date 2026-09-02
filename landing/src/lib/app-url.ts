const DEFAULT_APP_URL = 'http://localhost:5173';

export const APP_URL = import.meta.env.PUBLIC_APP_URL || DEFAULT_APP_URL;

/**
 * Help and download docs link the web app with root-relative `/app/...` hrefs,
 * which are only meaningful once rewritten to the app's own origin. Matching on
 * the prefix rather than the exact string keeps deep links such as
 * `/app/devices?add` working.
 */
export function applyAppLinks(html: string): string {
  return html.replace(/href="\/app(?=["/?#])/g, () => `href="${APP_URL}`);
}
