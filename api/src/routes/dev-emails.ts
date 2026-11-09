import { Hono } from 'hono';
import { buildEmailSamples, EmailSample } from '../lib/email/samples';
import { Env, Variables } from '../types/bindings';

const devEmails = new Hono<{ Bindings: Env; Variables: Variables }>();

function escapeHtml(value: string) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function snippet(text: string) {
  return text.replace(/\s+/g, ' ').trim().slice(0, 90);
}

function renderPage(input: {
  samples: EmailSample[];
  selected: EmailSample;
  view: 'html' | 'text';
  width: 'desktop' | 'mobile';
  from: string;
}) {
  const { samples, selected, view, width, from } = input;
  const href = (params: { t?: string; view?: string; width?: string }) =>
    `?${new URLSearchParams({
      t: params.t ?? selected.id,
      view: params.view ?? view,
      width: params.width ?? width,
    })}`;
  const tab = (label: string, current: boolean, to: string) =>
    `<a class="tab${current ? ' on' : ''}" href="${escapeHtml(to)}">${label}</a>`;

  const rows = samples
    .map(
      (sample) => `<a class="row${sample.id === selected.id ? ' on' : ''}" href="${escapeHtml(
        href({ t: sample.id }),
      )}">
        <span class="from">${escapeHtml(sample.label)}</span>
        <span class="subject">${escapeHtml(sample.email.subject)}</span>
        <span class="snippet">${escapeHtml(snippet(sample.email.text))}</span>
      </a>`,
    )
    .join('');

  const rawUrl = (kind: 'html' | 'text') =>
    `?${new URLSearchParams({ t: selected.id, raw: kind })}`;
  const body =
    view === 'html'
      ? `<iframe class="body" sandbox="allow-same-origin" src="${escapeHtml(rawUrl('html'))}" title="Email body" onload="try{this.style.height=this.contentDocument.documentElement.scrollHeight+'px'}catch(e){}"></iframe>`
      : `<pre class="body text">${escapeHtml(selected.email.text)}</pre>`;

  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>Email previews</title>
<style>
  * { box-sizing: border-box; }
  body { margin: 0; height: 100vh; display: flex; font: 14px/1.4 system-ui, sans-serif; color: #202124; background: #f6f8fc; }
  nav { width: 340px; flex: none; overflow-y: auto; background: #fff; border-right: 1px solid #dadce0; }
  nav h1 { margin: 0; padding: 14px 16px; font-size: 15px; border-bottom: 1px solid #dadce0; position: sticky; top: 0; background: #fff; }
  nav h1 small { display: block; margin-top: 2px; font-weight: 400; color: #5f6368; font-size: 12px; }
  .row { display: block; padding: 10px 16px; border-bottom: 1px solid #eee; color: inherit; text-decoration: none; }
  .row:hover { background: #f2f6fc; }
  .row.on { background: #d3e3fd; }
  .row span { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .from { font-weight: 600; font-size: 12px; color: #5f6368; text-transform: uppercase; letter-spacing: 0.03em; }
  .subject { font-weight: 600; }
  .snippet { color: #5f6368; font-size: 13px; }
  main { flex: 1; min-width: 0; overflow-y: auto; padding: 16px; }
  .toolbar { display: flex; gap: 6px; margin-bottom: 12px; flex-wrap: wrap; }
  .tab { padding: 5px 12px; border-radius: 16px; border: 1px solid #dadce0; background: #fff; color: #202124; text-decoration: none; font-size: 13px; }
  .tab.on { background: #202124; color: #fff; border-color: #202124; }
  .gap { flex: 1; }
  .message { margin: 0 auto; background: #fff; border-radius: 8px; box-shadow: 0 1px 3px rgba(60, 64, 67, 0.25); overflow: hidden; }
  .message.desktop { max-width: 820px; }
  .message.mobile { max-width: 390px; }
  .head { padding: 16px 20px; border-bottom: 1px solid #eee; }
  .head h2 { margin: 0 0 10px; font-size: 20px; font-weight: 500; }
  .who { display: flex; gap: 10px; align-items: center; }
  .avatar { width: 36px; height: 36px; border-radius: 50%; background: #1e3a2e; color: #fbf7ea; display: grid; place-items: center; font-weight: 600; flex: none; }
  .who b { display: block; }
  .who small { color: #5f6368; }
  iframe.body { display: block; width: 100%; min-height: 480px; border: 0; background: #fff; }
  pre.body { margin: 0; padding: 20px; white-space: pre-wrap; word-break: break-word; font: 13px/1.5 ui-monospace, monospace; }
  @media (max-width: 760px) { body { flex-direction: column; height: auto; } nav { width: auto; max-height: 40vh; } }
</style>
</head>
<body>
<nav>
  <h1>Email previews<small>${samples.length} templates, made-up data</small></h1>
  ${rows}
</nav>
<main>
  <div class="toolbar">
    ${tab('HTML', view === 'html', href({ view: 'html' }))}
    ${tab('Plain text', view === 'text', href({ view: 'text' }))}
    <span class="gap"></span>
    ${tab('Desktop', width === 'desktop', href({ width: 'desktop' }))}
    ${tab('Mobile', width === 'mobile', href({ width: 'mobile' }))}
  </div>
  <div class="message ${width}">
    <div class="head">
      <h2>${escapeHtml(selected.email.subject)}</h2>
      <div class="who">
        <span class="avatar">${escapeHtml(from.trim().charAt(0).toUpperCase() || '?')}</span>
        <span><b>${escapeHtml(from)}</b><small>to ${escapeHtml(selected.to)}</small></span>
      </div>
    </div>
    ${body}
  </div>
</main>
</body>
</html>`;
}

// API-066: previews every email template. Only a local dev environment, which logs email
// instead of sending it, serves this.
devEmails.get('/emails', (c) => {
  if (c.env.EMAIL_DELIVERY_MODE !== 'log') {
    return c.json({ error: 'Not found' }, 404);
  }

  const samples = buildEmailSamples(c.env);
  const selected = samples.find((sample) => sample.id === c.req.query('t')) ?? samples[0]!;

  const raw = c.req.query('raw');
  if (raw === 'html') return c.html(selected.email.html);
  if (raw === 'text') return c.text(selected.email.text);

  const appName = c.env.APP_NAME.trim().replace(/^the\s+/i, '');
  const from = `The ${appName} <${c.env.AWS_SES_FROM_EMAIL || 'noreply@example.com'}>`;
  return c.html(
    renderPage({
      samples,
      selected,
      view: c.req.query('view') === 'text' ? 'text' : 'html',
      width: c.req.query('width') === 'mobile' ? 'mobile' : 'desktop',
      from,
    }),
  );
});

export default devEmails;
