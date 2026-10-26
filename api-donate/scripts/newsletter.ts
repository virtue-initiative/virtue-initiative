/**
 * Sends a markdown file as a newsletter to confirmed subscribers, optionally filtered by where
 * they signed up from. Run from api-donate/ (see README.md, "Sending a newsletter").
 *
 * Subscribers are read straight from the worker's D1 with wrangler, and mail goes out through
 * SES using the same signed unsubscribe links the worker verifies. A dry run is the default and
 * `--send` asks for confirmation. Each address a `--send` reaches is appended to
 * `<file>.sent.jsonl`, so rerunning after a crash skips them instead of mailing them twice.
 */
import { execFileSync } from 'node:child_process';
import { appendFileSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import { createInterface } from 'node:readline/promises';
import { parseArgs } from 'node:util';
import { marked } from 'marked';
import { layout, markdownToText, newsletterFooter } from '../src/lib/newsletter-email';
import { signNewsletterToken } from '../src/lib/newsletter-token';
import { sendSesEmail, SesConfig } from '../src/lib/ses';

const USAGE = `Usage: bun run newsletter <file.md> [options]

The markdown file starts with front matter holding the subject:

  ---
  subject: October update
  ---

Audience (always confirmed, not unsubscribed, and not suppressed by a bounce or complaint):
  --country US        ISO country code
  --region CA         ISO 3166-2 subdivision code, e.g. a US state. Needs --country, since
                      codes repeat across countries (WA is Washington and Western Australia)
  --source landing    "landing" (newsletter form) or "signup" (account signup)
  --since 2026-01-01  signed up on or after this date
  --before 2026-06-01 signed up before this date

Mode (default is a dry run that writes <file>.preview.html and sends nothing):
  --test-to a@b.com   send only to this address
  --send              send to the whole audience, after a confirmation prompt
  --yes               skip that prompt

A --send records each address it reaches in <file>.sent.jsonl and skips those on a rerun, so a
crashed run can be resumed. Delete that file to mail the whole audience again.

Target:
  --env staging|prod  which D1 and unsubscribe host to use (default staging)
  --local             read the local D1 and use http://localhost:8788

Environment: AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_SES_REGION (default us-east-1),
NEWSLETTER_FROM_EMAIL, NEWSLETTER_TOKEN_SECRET (the target worker's value), and optionally
NEWSLETTER_SES_CONFIG_SET (default "newsletter"), NEWSLETTER_WORKER_URL (to point at a dev worker on another port),
NEWSLETTER_REPLY_TO (default
help@virtueinitiative.org) and NEWSLETTER_POSTAL_ADDRESS (defaults to the organization's address).
`;

const WORKER_URLS = {
  prod: 'https://donate-api.virtueinitiative.org',
  staging: 'https://staging.donate-api.virtueinitiative.org',
  local: 'http://localhost:8788',
} as const;

// Stays under SES's default 14 messages per second.
const SEND_INTERVAL_MS = 100;

function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

function requireEnv(name: string): string {
  return process.env[name]?.trim() || fail(`${name} is not set.`);
}

/** Filter values go into SQL text, so only allow plain characters. */
function sqlLiteral(flag: string, value: string): string {
  if (!/^[\w .'-]+$/.test(value)) fail(`Invalid value for --${flag}: ${value}`);
  return `'${value.replace(/'/g, "''")}'`;
}

function toUnix(flag: string, value: string): number {
  const ms = Date.parse(`${value}T00:00:00Z`);
  if (Number.isNaN(ms)) fail(`--${flag} must look like 2026-01-31.`);
  return Math.floor(ms / 1000);
}

function parseNewsletter(source: string): { subject: string; markdown: string } {
  const match = source.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/);
  const subject = match?.[1]
    ?.match(/^subject:\s*(.+)$/m)?.[1]
    ?.trim()
    .replace(/^(["'])(.*)\1$/, '$2');
  if (!match || !subject) fail('The file needs front matter with a "subject:" line.');
  return { subject, markdown: match[2]!.trim() };
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
  const body = marked.parse(markdown, { async: false }) as string;
  return body.replace(/<(a|img|h1|h2|h3|blockquote|hr|li)(?=[\s>/])/g, (_, tag: string) => {
    return `<${tag} style="${TAG_STYLES[tag]}"`;
  });
}

function queryRecipients(args: { env: string; local: boolean; where: string[] }): string[] {
  const sql = `SELECT email FROM newsletter_subscribers WHERE ${args.where.join(' AND ')} ORDER BY email`;
  const wranglerArgs = [
    'wrangler',
    'd1',
    'execute',
    'DB',
    '--env',
    args.env,
    args.local ? '--local' : '--remote',
    '--json',
    '--command',
    sql,
  ];
  const output = execFileSync('bunx', wranglerArgs, { encoding: 'utf8', maxBuffer: 64 << 20 });
  const parsed = JSON.parse(output.slice(output.indexOf('['))) as {
    results: { email: string }[];
  }[];
  return parsed.flatMap((statement) => statement.results.map((row) => row.email));
}

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    country: { type: 'string' },
    region: { type: 'string' },
    source: { type: 'string' },
    since: { type: 'string' },
    before: { type: 'string' },
    'test-to': { type: 'string' },
    send: { type: 'boolean', default: false },
    yes: { type: 'boolean', default: false },
    env: { type: 'string', default: 'staging' },
    local: { type: 'boolean', default: false },
    help: { type: 'boolean', short: 'h', default: false },
  },
});

const file = positionals[0];
if (values.help || !file) {
  console.log(USAGE);
  process.exit(values.help ? 0 : 1);
}
if (values.env !== 'staging' && values.env !== 'prod') fail('--env must be staging or prod.');
if (values.region && !values.country)
  fail('--region needs --country, since region codes repeat across countries.');
if (values.source && values.source !== 'landing' && values.source !== 'signup') {
  fail('--source must be landing or signup.');
}

const { subject, markdown } = parseNewsletter(readFileSync(file, 'utf8'));
const bodyHtml = renderHtml(markdown);

const where = ['confirmed_at IS NOT NULL', 'unsubscribed_at IS NULL', 'suppressed_at IS NULL'];
if (values.country) where.push(`country = ${sqlLiteral('country', values.country.toUpperCase())}`);
if (values.region) where.push(`region_code = ${sqlLiteral('region', values.region.toUpperCase())}`);
if (values.source) where.push(`source = ${sqlLiteral('source', values.source)}`);
if (values.since) where.push(`created_at >= ${toUnix('since', values.since)}`);
if (values.before) where.push(`created_at < ${toUnix('before', values.before)}`);

const workerUrl =
  process.env.NEWSLETTER_WORKER_URL?.trim() ||
  (values.local ? WORKER_URLS.local : WORKER_URLS[values.env]);
const audience = queryRecipients({ env: values.env, local: values.local, where });
const recipients = values['test-to'] ? [values['test-to'].trim().toLowerCase()] : audience;

console.log(`Subject: ${subject}`);
console.log(`Audience: ${audience.length} confirmed subscriber(s) match the filters.`);
if (values['test-to']) console.log(`Test send to ${recipients[0]} only.`);

// Shown in every footer, as CAN-SPAM asks. NEWSLETTER_POSTAL_ADDRESS overrides it.
const POSTAL_ADDRESS = '75 Knapp Rd, Mechanicville, NY 12118';
const postalAddress = process.env.NEWSLETTER_POSTAL_ADDRESS?.trim() || POSTAL_ADDRESS;

async function buildMessage(email: string) {
  const secret = requireEnv('NEWSLETTER_TOKEN_SECRET');
  const unsubscribeUrl = `${workerUrl}/newsletter/unsubscribe?token=${await signNewsletterToken(secret, 'unsubscribe', email)}`;
  const footer = newsletterFooter(unsubscribeUrl, postalAddress);
  return {
    from: requireEnv('NEWSLETTER_FROM_EMAIL'),
    configurationSet: process.env.NEWSLETTER_SES_CONFIG_SET?.trim() || 'newsletter',
    to: email,
    replyTo: process.env.NEWSLETTER_REPLY_TO?.trim() || 'help@virtueinitiative.org',
    subject,
    text: `${markdownToText(markdown)}\n${footer.text}`,
    html: layout(bodyHtml, footer.html),
    headers: {
      'List-Unsubscribe': `<${unsubscribeUrl}>`,
      'List-Unsubscribe-Post': 'List-Unsubscribe=One-Click',
    },
  };
}

if (!values.send && !values['test-to']) {
  const previewPath = file.replace(/\.md$/, '') + '.preview.html';
  const footer = newsletterFooter(
    `${workerUrl}/newsletter/unsubscribe?token=preview`,
    postalAddress,
  );
  writeFileSync(previewPath, layout(bodyHtml, footer.html));
  console.log(`Sample: ${audience.slice(0, 10).join(', ')}${audience.length > 10 ? ', ...' : ''}`);
  console.log(`Wrote ${previewPath}. Dry run, nothing sent. Use --test-to or --send.`);
  process.exit(0);
}

const ses: SesConfig = {
  region: process.env.AWS_SES_REGION?.trim() || 'us-east-1',
  accessKeyId: requireEnv('AWS_ACCESS_KEY_ID'),
  secretAccessKey: requireEnv('AWS_SECRET_ACCESS_KEY'),
};

// A wrong NEWSLETTER_TOKEN_SECRET (say the staging value with --env prod) would put an
// unsubscribe link that returns 400 in every email, so try the first link before sending anything.
async function assertUnsubscribeLinkWorks(email: string) {
  const header = (await buildMessage(email)).headers['List-Unsubscribe'];
  const url = header.slice(1, -1);
  try {
    const res = await fetch(url);
    if (!res.ok) {
      fail(
        `The unsubscribe link for ${email} returned ${res.status} from ${workerUrl}. ` +
          'NEWSLETTER_TOKEN_SECRET probably does not match that worker. Nothing was sent.',
      );
    }
  } catch (err) {
    // The local worker is often not running, which is fine for a local trial.
    if (!values.local) fail(`Could not reach ${workerUrl} to check the unsubscribe link: ${err}`);
    console.warn(`Skipped the unsubscribe link check, ${workerUrl} is not reachable.`);
  }
}

// Only a full --send keeps a log. A test send to one address is not part of the issue's history.
// Entries name the worker they went through, so a staging rehearsal doesn't skip anyone in prod.
const keepLog = values.send && !values['test-to'];
const logPath = file.replace(/\.md$/, '') + '.sent.jsonl';
const alreadySent = new Set<string>();
if (keepLog && existsSync(logPath)) {
  for (const line of readFileSync(logPath, 'utf8').split('\n')) {
    if (!line.trim()) continue;
    const entry = JSON.parse(line) as { email: string; status: string; target?: string };
    if (entry.status === 'sent' && entry.target === workerUrl) alreadySent.add(entry.email);
  }
}
const toSend = recipients.filter((email) => !alreadySent.has(email));
if (alreadySent.size > 0) {
  console.log(`Skipping ${recipients.length - toSend.length} address(es) already in ${logPath}.`);
}
if (toSend.length === 0) {
  console.log('Nobody left to send to.');
  process.exit(0);
}

await assertUnsubscribeLinkWorks(toSend[0]!);

if (values.send && !values.yes) {
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  const answer = await rl.question(
    `Send to ${toSend.length} recipient(s) via ${values.env}? [y/N] `,
  );
  rl.close();
  if (answer.trim().toLowerCase() !== 'y') fail('Cancelled.');
}

const record = (email: string, status: 'sent' | 'failed', error?: string) => {
  if (!keepLog) return;
  const entry = { email, status, target: workerUrl, at: new Date().toISOString(), ...(error ? { error } : {}) };
  appendFileSync(logPath, JSON.stringify(entry) + '\n');
};

const failed: string[] = [];
for (const [i, email] of toSend.entries()) {
  try {
    await sendSesEmail(ses, await buildMessage(email));
    record(email, 'sent');
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    failed.push(email);
    record(email, 'failed', message);
    console.error(`Failed ${email}: ${message}`);
  }
  if ((i + 1) % 50 === 0) console.log(`${i + 1}/${toSend.length}`);
  await new Promise((resolve) => setTimeout(resolve, SEND_INTERVAL_MS));
}

console.log(`Sent ${toSend.length - failed.length} of ${toSend.length}.`);
if (failed.length > 0) {
  console.log(`Failed addresses (run again to retry only these):\n${failed.join('\n')}`);
  process.exit(1);
}
