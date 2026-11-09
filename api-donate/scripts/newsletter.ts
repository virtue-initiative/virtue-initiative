/**
 * Newsletter CLI. Reads and writes the `newsletter_signups` table in this worker's D1 with
 * wrangler, and sends email through SES. Run from api-donate/ (see README.md, "Newsletter").
 */
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, relative, resolve } from 'node:path';
import { createInterface } from 'node:readline/promises';
import { parseArgs } from 'node:util';
import { SESv2Client, SendEmailCommand } from '@aws-sdk/client-sesv2';
import { marked } from 'marked';
import { d1, fail, parseEnv, sqlString } from './d1';

const USAGE = `Usage:
  bun run newsletter send <file.md> --env staging|prod|local [--country US] [--region CA]
      Writes .newsletter/<name>.html as a preview and prints how many subscribers match. Nothing
      is sent. A blog post (landing/src/content/blog) uses its title as the subject, and its
      relative links and images point at the published post. Any other file needs a first
      "# Heading", which is the subject.
    --to you@example.com   Send only to this address (it need not be a subscriber).
    --send                 Send to every confirmed, subscribed match, after a prompt. Addresses
                           are logged to .newsletter/<name>.sent.txt and skipped on a rerun.
  bun run newsletter unsubscribe <email> --env staging|prod|local
      For someone who asks by email instead of using the link.

Sending needs AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY for the newsletter SES user.
--env local reads the local API's D1. Set NEWSLETTER_LANDING_URL and NEWSLETTER_API_URL to the
ports launch.sh printed, so the unsubscribe links point at them.`;

const FROM = 'The Virtue Initiative <newsletter@news.virtueinitiative.org>';
const REPLY_TO = 'help@virtueinitiative.org';
const SES_REGION = 'us-east-1';
// SES configuration set for newsletter mail. Its event destination sends bounces and complaints
// to the API's /email/sns, which unsubscribes those addresses (API-041). The API recognises
// newsletter events by this name.
const SES_CONFIG_SET = 'newsletter';
const POSTAL_ADDRESS = '75 Knapp Rd, Mechanicville, NY 12118';
const SENDS_PER_SECOND = 10;
const BLOG_DIR = resolve(import.meta.dir, '../../landing/src/content/blog');
// Previews and sent logs, kept out of the blog folder and out of git (they list subscribers).
const OUT_DIR = resolve(import.meta.dir, '../.newsletter');
const URLS = {
  prod: { landing: 'https://virtueinitiative.org', api: 'https://app.virtueinitiative.org/api' },
  staging: {
    landing: 'https://staging.virtueinitiative.org',
    api: 'https://staging.app.virtueinitiative.org/api',
  },
  // launch.sh picks random ports, so set NEWSLETTER_LANDING_URL and NEWSLETTER_API_URL to match.
  local: { landing: 'http://localhost:4321', api: 'http://localhost:8787' },
};

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
const env = parseEnv(values.env, USAGE);
const urls = {
  landing: process.env.NEWSLETTER_LANDING_URL || URLS[env].landing,
  api: process.env.NEWSLETTER_API_URL || URLS[env].api,
};

/** Unsubscribe links for one subscriber's token (API-060). */
function unsubscribeUrls(token: string) {
  const query = `?token=${encodeURIComponent(token)}`;
  return {
    page: `${urls.landing}/newsletter/unsubscribe${query}`,
    // Unversioned, so it keeps working in old emails after the API version changes.
    oneClick: `${urls.api}/newsletter/unsubscribe${query}`,
  };
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

interface Issue {
  subject: string;
  /** The body, starting with the subject as a "# Heading". */
  markdown: string;
  /** Relative links and images resolve against this. */
  baseUrl: string;
  /** The published blog post, when the issue is one. */
  postUrl?: string;
  /** Names the preview and sent log in OUT_DIR. */
  name: string;
}

/**
 * Reads an issue. A blog post's frontmatter gives the subject, and its folder's images are
 * published beside the post at /blog/<slug>/<file> (landing/blog-assets.mjs), so `./image.png`
 * resolves there.
 */
function readIssue(path: string): Issue {
  const text = readFileSync(path, 'utf8').trim();
  const frontmatter = text.match(/^---\n([\s\S]*?)\n---\n?/);
  const meta: Record<string, string> = {};
  for (const line of frontmatter?.[1].split('\n') ?? []) {
    const field = line.match(/^(\w+):\s*(.*)$/);
    if (field) meta[field[1]] = field[2].replace(/^(['"])(.*)\1$/, '$2').trim();
  }
  const body = text.slice(frontmatter?.[0].length ?? 0).trim();

  const fromBlog = relative(BLOG_DIR, resolve(path));
  if (!fromBlog.startsWith('..') && /\.mdx?$/.test(fromBlog)) {
    if (meta.draft === 'true') fail('This post is a draft, so it is not on the website yet.');
    if (!meta.title) fail('The post needs a title.');
    const inFolder = /^index\.mdx?$/.test(basename(fromBlog));
    const slug = inFolder ? basename(dirname(fromBlog)) : basename(fromBlog).replace(/\.mdx?$/, '');
    const postUrl = `${urls.landing}/blog/${slug}`;
    return {
      subject: meta.title,
      markdown: `# ${meta.title}\n\n${body}`,
      baseUrl: inFolder ? `${postUrl}/` : `${urls.landing}/blog/`,
      postUrl,
      name: slug,
    };
  }

  const subject = body.match(/^#\s+(.+)$/m)?.[1]?.trim();
  if (!subject) fail('The file needs a "# Heading" line to use as the subject.');
  return {
    subject,
    markdown: body,
    baseUrl: `${urls.landing}/`,
    name: basename(path).replace(/\.mdx?$/, ''),
  };
}

/** Makes a link absolute, since an email has no page for a relative one to be relative to. */
function absoluteUrl(url: string, baseUrl: string): string {
  try {
    return new URL(url, baseUrl).href;
  } catch {
    return url;
  }
}

function renderBodyHtml(markdown: string, baseUrl: string): string {
  return (marked.parse(markdown, { async: false }) as string)
    .replace(
      /<(a|img|h1|h2|h3|blockquote|hr|li)(?=[\s>/])/g,
      (_, tag: string) => `<${tag} style="${TAG_STYLES[tag]}"`,
    )
    .replace(
      /\b(href|src)="([^"]*)"/g,
      (_, attr: string, url: string) => `${attr}="${absoluteUrl(url, baseUrl)}"`,
    );
}

function renderHtml(body: string, unsubscribeUrl: string, postUrl?: string): string {
  return `<!doctype html>
<html lang="en">
<body style="margin:0;padding:24px 12px;background:#f4f4f1;">
<div style="max-width:600px;margin:0 auto;padding:28px;background:#ffffff;border-radius:8px;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif;font-size:16px;line-height:1.6;color:#1c1c1c;">
${body}
<hr style="border:none;border-top:1px solid #e3e3df;margin:28px 0 16px;">
<div style="font-size:13px;color:#6b6b66;">
${postUrl ? `<p style="margin:0 0 8px;">You can also <a href="${postUrl}" style="color:#6b6b66;">read this on our blog</a>.</p>\n` : ''}<p style="margin:0 0 8px;">You got this email because you subscribed to Virtue Initiative project updates.</p>
<p style="margin:0 0 8px;">${POSTAL_ADDRESS}</p>
<p style="margin:0;"><a href="${unsubscribeUrl}" style="color:#6b6b66;">Unsubscribe</a></p>
</div>
</div>
</body>
</html>
`;
}

/** The text/plain part: links become `text (url)`, and heading and emphasis markers are dropped. */
function renderTextBody(markdown: string, baseUrl: string): string {
  return markdown
    .replace(/!?\[([^\]]*)\]\(([^)\s]+)[^)]*\)/g, (_, text: string, path: string) => {
      const url = absoluteUrl(path, baseUrl);
      return text && text !== path ? `${text} (${url})` : url;
    })
    .replace(/^#{1,6}\s+/gm, '')
    .replace(/(\*\*|__)(.+?)\1/g, '$2');
}

function renderText(body: string, unsubscribeUrl: string, postUrl?: string): string {
  const post = postUrl ? `You can also read this on our blog: ${postUrl}\n` : '';
  return `${body}\n\n--\n${post}You got this email because you subscribed to Virtue Initiative project updates.\n${POSTAL_ADDRESS}\nUnsubscribe: ${unsubscribeUrl}\n`;
}

async function send() {
  if (!arg || !existsSync(arg)) fail(USAGE);
  const { subject, markdown, baseUrl, postUrl, name } = readIssue(arg);
  const bodyHtml = renderBodyHtml(markdown, baseUrl);
  const bodyText = renderTextBody(markdown, baseUrl);

  mkdirSync(OUT_DIR, { recursive: true });
  const htmlPath = resolve(OUT_DIR, `${name}.html`);
  writeFileSync(htmlPath, renderHtml(bodyHtml, unsubscribeUrls('preview').page, postUrl));
  console.log(`Subject: ${subject}\nPreview: ${htmlPath}`);

  const accessKeyId = process.env.AWS_ACCESS_KEY_ID;
  const secretAccessKey = process.env.AWS_SECRET_ACCESS_KEY;
  const ses = () => {
    if (!accessKeyId || !secretAccessKey) fail('Set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY.');
    return new SESv2Client({ region: SES_REGION, credentials: { accessKeyId, secretAccessKey } });
  };
  const sendTo = (client: SESv2Client, to: string, token: string) => {
    const unsubscribe = unsubscribeUrls(token);
    return client.send(
      new SendEmailCommand({
        FromEmailAddress: FROM,
        ReplyToAddresses: [REPLY_TO],
        ConfigurationSetName: SES_CONFIG_SET,
        Destination: { ToAddresses: [to] },
        Content: {
          Simple: {
            Subject: { Data: subject },
            Body: {
              Html: { Data: renderHtml(bodyHtml, unsubscribe.page, postUrl) },
              Text: { Data: renderText(bodyText, unsubscribe.page, postUrl) },
            },
            // RFC 8058 one-click unsubscribe (API-060).
            Headers: [
              { Name: 'List-Unsubscribe', Value: `<${unsubscribe.oneClick}>` },
              { Name: 'List-Unsubscribe-Post', Value: 'List-Unsubscribe=One-Click' },
            ],
          },
        },
      }),
    );
  };

  if (values.to) {
    const to = values.to.trim().toLowerCase();
    const [row] = d1<{ token: string }>(
      env,
      `SELECT token FROM newsletter_signups WHERE email = ${sqlString(to)};`,
    );
    if (!row) console.log(`${to} is not on the list, so its unsubscribe link won't work.`);
    await sendTo(ses(), to, row?.token ?? 'preview');
    console.log(`Sent to ${to}.`);
    return;
  }

  const where = ['confirmed_at IS NOT NULL', 'unsubscribed_at IS NULL'];
  for (const key of ['country', 'region'] as const) {
    const value = values[key]?.toUpperCase();
    if (!value) continue;
    if (!/^[A-Z0-9]{1,3}$/.test(value)) fail(`--${key} must be a code like US or CA.`);
    where.push(`${key} = ${sqlString(value)}`);
  }
  const rows = d1<{ email: string; token: string }>(
    env,
    `SELECT email, token FROM newsletter_signups WHERE ${where.join(' AND ')} ORDER BY email;`,
  );

  const sentLog = resolve(OUT_DIR, `${name}.sent.txt`);
  const alreadySent = new Set(
    existsSync(sentLog) ? readFileSync(sentLog, 'utf8').split('\n').filter(Boolean) : [],
  );
  const audience = rows.filter((row) => !alreadySent.has(row.email));
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
  for (const { email, token } of audience) {
    try {
      await sendTo(client, email, token);
      appendFileSync(sentLog, `${email}\n`);
    } catch (err) {
      failed++;
      console.error(`Failed to send to ${email}:`, err);
    }
    await new Promise((resolve) => setTimeout(resolve, 1000 / SENDS_PER_SECOND));
  }
  console.log(`Sent ${audience.length - failed}, failed ${failed}.`);
}

function unsubscribe() {
  if (!arg) fail(USAGE);
  const email = arg.trim().toLowerCase();
  const [row] = d1<{ email: string }>(
    env,
    `UPDATE newsletter_signups SET unsubscribed_at = unixepoch() WHERE email = ${sqlString(email)} RETURNING email;`,
  );
  console.log(row ? `Unsubscribed ${email}.` : `${email} is not on the list.`);
}

if (command === 'send') await send();
else if (command === 'unsubscribe') unsubscribe();
else fail(USAGE);
