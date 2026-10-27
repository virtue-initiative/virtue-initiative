/**
 * One-time copy of Buttondown's active subscribers into `newsletter_signups`. Run from
 * api-donate/ (see README.md, "Newsletter"). Safe to rerun: an address already in the table keeps
 * its row, so nobody is resubscribed and no location is overwritten.
 */
import { parseArgs } from 'node:util';
import { d1, fail, parseEnv, sqlString } from './d1';

const USAGE = `Usage: BUTTONDOWN_API_KEY=... bun run import-buttondown --env staging|prod|local`;

const { values } = parseArgs({ options: { env: { type: 'string' } } });
const env = parseEnv(values.env, USAGE);
const apiKey = process.env.BUTTONDOWN_API_KEY;
if (!apiKey) fail(USAGE);

const subscribers: { email: string; createdAt: number }[] = [];
let url: string | null = 'https://api.buttondown.com/v1/subscribers?type=regular';
while (url) {
  const res = await fetch(url, { headers: { Authorization: `Token ${apiKey}` } });
  if (!res.ok) fail(`Buttondown returned ${res.status}: ${await res.text()}`);
  const page = (await res.json()) as {
    results: { email_address: string; type: string; creation_date: string }[];
    next: string | null;
  };
  for (const s of page.results) {
    if (s.type !== 'regular') continue;
    subscribers.push({
      email: s.email_address.trim().toLowerCase(),
      createdAt: Math.floor(Date.parse(s.creation_date) / 1000),
    });
  }
  url = page.next;
}

// They confirmed with Buttondown, so they come in confirmed as of when they subscribed.
const rows = subscribers.map((s) => {
  const at = s.createdAt || 'unixepoch()';
  return `(${sqlString(s.email)}, ${at}, ${at})`;
});
if (rows.length > 0) {
  d1(
    env,
    `INSERT OR IGNORE INTO newsletter_signups (email, created_at, confirmed_at) VALUES ${rows.join(', ')};`,
  );
}
const [count] = d1<{ n: number }>(env, 'SELECT COUNT(*) AS n FROM newsletter_signups;');
console.log(
  `Imported ${subscribers.length} Buttondown subscribers. The table has ${count?.n} rows.`,
);
