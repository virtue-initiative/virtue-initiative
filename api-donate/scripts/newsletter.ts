/**
 * Newsletter CLI. Reads and writes the `newsletter_signups` table in this worker's D1 with
 * wrangler, and sends email through SES. Run from api-donate/ (see README.md, "Newsletter").
 */
import { execFileSync } from 'node:child_process';
import { appendFileSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline/promises';
import { parseArgs } from 'node:util';
import { SESv2Client, SendEmailCommand } from '@aws-sdk/client-sesv2';
import { marked } from 'marked';

const USAGE = `Usage:
  bun run newsletter send <file.md> --env staging|prod|local [--country US] [--region CA]
      Writes <file>.html as a preview and prints how many subscribers match. Nothing is sent.
      The file's first "# Heading" is the subject.
    --to you@example.com   Send only to this address (it need not be a subscriber).
    --send                 Send to every matching subscriber, after a prompt. Addresses are logged
                           to <file>.sent.txt and skipped on a rerun.
  bun run newsletter import-buttondown --env staging|prod|local
      Copies Buttondown's active subscribers (BUTTONDOWN_API_KEY) into the table.
  bun run newsletter unsubscribe <email> --env staging|prod|local

Sending needs AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY for the newsletter SES user.`;

const FROM = 'The Virtue Initiative <newsletter@news.virtueinitiative.org>';
const REPLY_TO = 'help@virtueinitiative.org';
const SES_REGION = 'us-east-1';
// SES configuration set for newsletter mail, so its bounces and complaints stay apart from
// account email.
const SES_CONFIG_SET = 'newsletter';
const POSTAL_ADDRESS = '75 Knapp Rd, Mechanicville, NY 12118';
const UNSUBSCRIBE_MAILTO = `mailto:${REPLY_TO}?subject=Unsubscribe`;
const SENDS_PER_SECOND = 10;

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    env: { type: 'string' },
    country: { type: 'string' },
    region: { type: 'string' },
    to: { type: 'string' },
    send: { type: 'boolean', default: false },
  },
});
const [command, arg] = positionals;

function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

if (values.env !== 'staging' && values.env !== 'prod' && values.env !== 'local') fail(USAGE);
const env = values.env;
// `local` is the local D1 that `bun run dev` uses.
const wranglerTarget =
  env === 'local' ? ['--env', 'staging', '--local'] : ['--env', env, '--remote'];

function sqlString(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}

/** Runs SQL against the remote D1 of `env` and returns the rows of the last statement. */
function d1<T>(sql: string): T[] {
  const sqlFile = join(tmpdir(), `newsletter-${process.pid}.sql`);
  writeFileSync(sqlFile, sql);
  const out = execFileSync(
    'bunx',
    ['wrangler', 'd1', 'execute', 'DB', ...wranglerTarget, '--json', '--file', sqlFile],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], maxBuffer: 256 * 1024 * 1024 },
  );
  const results = JSON.parse(out) as { results: T[] }[];
  return results.at(-1)?.results ?? [];
}

// Inline styles, since many mail clients ignore <style> blocks.
const TAG_STYLES: Record<string, string> = {
  a: 'color:#1f4f46;',
  img: 'max-width:100%;height:auto;',
  h1: 'font-size:26px;line-height:1.25;margin:0 0 16px;',
  h2: 'font-size:20px;line-height:1.3;margin:28px 0 12px;',
  h3: 'font-size:17px;line-height:1.3;margin:24px 0 8px;',
  blockquote: 'margin:16px 0;padding:4px 16px;border-left:4px solid #d6d6d0;color:#55554f;',
  hr: 'border:none;border-top:1px solid #e3e3df;margin:24px 0;',
  li: 'margin:0 0 8px;',
};

function renderHtml(markdown: string): string {
  const body = (marked.parse(markdown, { async: false }) as string).replace(
    /<(a|img|h1|h2|h3|blockquote|hr|li)(?=[\s>/])/g,
    (_, tag: string) => `<${tag} style="${TAG_STYLES[tag]}"`,
  );
  return `<!doctype html>
<html lang="en">
<body style="margin:0;padding:24px 12px;background:#f4f4f1;">
<div style="max-width:600px;margin:0 auto;padding:28px;background:#ffffff;border-radius:8px;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif;font-size:16px;line-height:1.6;color:#1c1c1c;">
${body}
<hr style="border:none;border-top:1px solid #e3e3df;margin:28px 0 16px;">
<div style="font-size:13px;color:#6b6b66;">
<p style="margin:0 0 8px;">You got this email because you subscribed to Virtue Initiative project updates.</p>
<p style="margin:0 0 8px;">${POSTAL_ADDRESS}</p>
<p style="margin:0;"><a href="${UNSUBSCRIBE_MAILTO}" style="color:#6b6b66;">Unsubscribe</a> by replying with "unsubscribe".</p>
</div>
</div>
</body>
</html>
`;
}

/** The text/plain part: links become `text (url)`, and heading and emphasis markers are dropped. */
function renderText(markdown: string): string {
  const body = markdown
    .replace(/!?\[([^\]]*)\]\(([^)\s]+)[^)]*\)/g, (_, text: string, url: string) =>
      text && text !== url ? `${text} (${url})` : url,
    )
    .replace(/^#{1,6}\s+/gm, '')
    .replace(/(\*\*|__)(.+?)\1/g, '$2');
  return `${body}\n\n--\nYou got this email because you subscribed to Virtue Initiative project updates.\n${POSTAL_ADDRESS}\nTo unsubscribe, reply with "unsubscribe".\n`;
}

async function send() {
  if (!arg || !existsSync(arg)) fail(USAGE);
  const markdown = readFileSync(arg, 'utf8').trim();
  const subject = markdown.match(/^#\s+(.+)$/m)?.[1]?.trim();
  if (!subject) fail('The file needs a "# Heading" line to use as the subject.');
  const html = renderHtml(markdown);
  const text = renderText(markdown);

  const htmlPath = arg.replace(/\.md$/, '') + '.html';
  writeFileSync(htmlPath, html);
  console.log(`Subject: ${subject}\nPreview: ${htmlPath}`);

  const accessKeyId = process.env.AWS_ACCESS_KEY_ID;
  const secretAccessKey = process.env.AWS_SECRET_ACCESS_KEY;
  const ses = () => {
    if (!accessKeyId || !secretAccessKey) fail('Set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY.');
    return new SESv2Client({ region: SES_REGION, credentials: { accessKeyId, secretAccessKey } });
  };
  const sendTo = (client: SESv2Client, to: string) =>
    client.send(
      new SendEmailCommand({
        FromEmailAddress: FROM,
        ReplyToAddresses: [REPLY_TO],
        ConfigurationSetName: SES_CONFIG_SET,
        Destination: { ToAddresses: [to] },
        Content: {
          Simple: {
            Subject: { Data: subject },
            Body: { Html: { Data: html }, Text: { Data: text } },
            Headers: [{ Name: 'List-Unsubscribe', Value: `<${UNSUBSCRIBE_MAILTO}>` }],
          },
        },
      }),
    );

  if (values.to) {
    await sendTo(ses(), values.to);
    console.log(`Sent to ${values.to}.`);
    return;
  }

  const where = ['unsubscribed_at IS NULL'];
  for (const key of ['country', 'region'] as const) {
    const value = values[key]?.toUpperCase();
    if (!value) continue;
    if (!/^[A-Z0-9]{1,3}$/.test(value)) fail(`--${key} must be a code like US or CA.`);
    where.push(`${key} = ${sqlString(value)}`);
  }
  const rows = d1<{ email: string }>(
    `SELECT email FROM newsletter_signups WHERE ${where.join(' AND ')} ORDER BY email;`,
  );

  const sentLog = arg.replace(/\.md$/, '') + '.sent.txt';
  const alreadySent = new Set(
    existsSync(sentLog) ? readFileSync(sentLog, 'utf8').split('\n').filter(Boolean) : [],
  );
  const audience = rows.map((row) => row.email).filter((email) => !alreadySent.has(email));
  console.log(
    `${rows.length} subscribers match on ${env}` +
      (alreadySent.size ? `, ${rows.length - audience.length} already sent (${sentLog})` : '') +
      '.',
  );
  if (!values.send) {
    console.log('Dry run. Pass --to <address> to send a test, or --send to send to all.');
    return;
  }

  const rl = createInterface({ input: process.stdin, output: process.stdout });
  const answer = await rl.question(`Send "${subject}" to ${audience.length} addresses? [y/N] `);
  rl.close();
  if (answer.trim().toLowerCase() !== 'y') fail('Cancelled.');

  const client = ses();
  let failed = 0;
  for (const email of audience) {
    try {
      await sendTo(client, email);
      appendFileSync(sentLog, `${email}\n`);
    } catch (err) {
      failed++;
      console.error(`Failed to send to ${email}:`, err);
    }
    await new Promise((resolve) => setTimeout(resolve, 1000 / SENDS_PER_SECOND));
  }
  console.log(`Sent ${audience.length - failed}, failed ${failed}.`);
}

async function importButtondown() {
  const apiKey = process.env.BUTTONDOWN_API_KEY;
  if (!apiKey) fail('Set BUTTONDOWN_API_KEY.');

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

  // Existing rows win, so this never resubscribes anyone or overwrites a location.
  const values = subscribers.map((s) => `(${sqlString(s.email)}, ${s.createdAt || 'unixepoch()'})`);
  for (let i = 0; i < values.length; i += 500) {
    d1(
      `INSERT OR IGNORE INTO newsletter_signups (email, created_at) VALUES ${values.slice(i, i + 500).join(', ')};`,
    );
  }
  const [count] = d1<{ n: number }>('SELECT COUNT(*) AS n FROM newsletter_signups;');
  console.log(`Imported ${subscribers.length} Buttondown subscribers. The table has ${count?.n}.`);
}

function unsubscribe() {
  if (!arg) fail(USAGE);
  const email = arg.trim().toLowerCase();
  const [row] = d1<{ email: string }>(
    `UPDATE newsletter_signups SET unsubscribed_at = unixepoch() WHERE email = ${sqlString(email)} AND unsubscribed_at IS NULL RETURNING email;`,
  );
  console.log(row ? `Unsubscribed ${email}.` : `${email} is not subscribed.`);
}

if (command === 'send') await send();
else if (command === 'import-buttondown') await importButtondown();
else if (command === 'unsubscribe') unsubscribe();
else fail(USAGE);
