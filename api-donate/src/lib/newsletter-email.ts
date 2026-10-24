export function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

export const confirmationEmail = (confirmUrl: string) => ({
  subject: 'Confirm your Virtue Initiative newsletter subscription',
  text: `Someone, hopefully you, asked to get Virtue Initiative project updates at this address.\n\nConfirm your subscription: ${confirmUrl}\n\nIf this wasn't you, ignore this email and you won't hear from us again.\n`,
  html: layout(
    `<p>Someone, hopefully you, asked to get Virtue Initiative project updates at this address.</p>
<p><a href="${escapeHtml(confirmUrl)}" style="${BUTTON}">Confirm subscription</a></p>
<p>If this wasn't you, ignore this email and you won't hear from us again.</p>`,
  ),
});

const BUTTON =
  'display:inline-block;padding:10px 18px;background:#1f4f46;color:#ffffff;border-radius:6px;text-decoration:none;font-weight:600;';

/** Wraps rendered newsletter content in a simple, client-safe single-column email shell. */
export function layout(bodyHtml: string, footerHtml = ''): string {
  return `<!doctype html>
<html lang="en">
<body style="margin:0;padding:24px 12px;background:#f4f4f1;">
<div style="max-width:600px;margin:0 auto;padding:28px;background:#ffffff;border-radius:8px;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif;font-size:16px;line-height:1.6;color:#1c1c1c;">
${bodyHtml}
${footerHtml ? `<hr style="border:none;border-top:1px solid #e3e3df;margin:28px 0 16px;">\n<div style="font-size:13px;color:#6b6b66;">${footerHtml}</div>` : ''}
</div>
</body>
</html>
`;
}

export function newsletterFooter(
  unsubscribeUrl: string,
  postalAddress?: string,
): {
  html: string;
  text: string;
} {
  const address = postalAddress?.trim();
  return {
    html: `<p style="margin:0 0 8px;">You got this email because you subscribed to Virtue Initiative project updates.</p>${
      address ? `<p style="margin:0 0 8px;">${escapeHtml(address)}</p>` : ''
    }<p style="margin:0;"><a href="${escapeHtml(unsubscribeUrl)}" style="color:#6b6b66;">Unsubscribe</a></p>`,
    text: `\n--\nYou got this email because you subscribed to Virtue Initiative project updates.\n${
      address ? `${address}\n` : ''
    }Unsubscribe: ${unsubscribeUrl}\n`,
  };
}

/**
 * Turns the newsletter's markdown into readable plain text for the text/plain part: links become
 * `text (url)`, and heading, emphasis and code markers are dropped. Lists and quotes already read
 * fine as text, so they are left alone.
 */
export function markdownToText(markdown: string): string {
  return markdown
    .replace(/!\[([^\]]*)\]\(([^)\s]+)[^)]*\)/g, (_, alt: string, url: string) =>
      alt ? `${alt} (${url})` : url,
    )
    .replace(/\[([^\]]+)\]\(([^)\s]+)[^)]*\)/g, (_, text: string, url: string) =>
      text === url || `mailto:${text}` === url ? text : `${text} (${url})`,
    )
    .replace(/^#{1,6}\s+/gm, '')
    .replace(/(\*\*|__)(.+?)\1/g, '$2')
    .replace(/(?<![\w*])\*(?!\s)([^*\n]+?)\*(?![\w*])/g, '$1')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/[ \t]+$/gm, '');
}
