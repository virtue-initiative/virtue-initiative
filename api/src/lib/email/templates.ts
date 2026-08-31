import { DigestFrequency, TamperSeverity } from '../email-domain';

// Mirrors the app's warm institutional palette (shared-web/tokens.css) so
// transactional emails match the product's actual branding.
const EMAIL_COLORS = {
  text: '#1b1a16', // --text
  textMuted: '#6a6655', // --text-muted
  textOnAccent: '#fbf7ea', // --paper-3 (matches .vi-btn--primary's text color)
  accent: '#1e3a2e', // --accent / --forest
  pageBg: '#f4efe3', // --bg / --paper
  surface: '#fbf7ea', // --surface / --paper-3
  border: '#d9d1bc', // --border
} as const;

// Mirrors --font / --font-serif in shared-web/tokens.css. Custom web fonts
// aren't loaded in most email clients, so these fall back to the same
// generic stacks the app itself falls back to.
const EMAIL_FONTS = {
  sans: "'IBM Plex Sans', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif",
  serif: "'Source Serif 4', Georgia, serif",
} as const;

function inlineStyle(rules: Record<string, string>) {
  return Object.entries(rules)
    .map(([key, value]) => `${key}:${value}`)
    .join(';');
}

function themedLink(url: string, label: string) {
  return `<a href="${escapeHtml(url)}" style="${inlineStyle({
    color: EMAIL_COLORS.accent,
    'text-decoration': 'none',
  })}">${escapeHtml(label)}</a>`;
}

function escapeHtml(value: string) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function paragraph(text: string) {
  return `<p style="${inlineStyle({
    margin: '0 0 16px 0',
    color: EMAIL_COLORS.text,
    'font-size': '16px',
    'line-height': '1.5',
  })}">${escapeHtml(text)}</p>`;
}

// Mirrors .vi-btn--primary in shared-web/components/Button/Button.css.
function actionButton(url: string, label: string) {
  return `<p style="${inlineStyle({ margin: '0 0 16px 0' })}"><a href="${escapeHtml(url)}" style="${inlineStyle(
    {
      display: 'inline-block',
      background: EMAIL_COLORS.accent,
      color: EMAIL_COLORS.textOnAccent,
      'text-decoration': 'none',
      'font-weight': '500',
      'font-size': '14px',
      'letter-spacing': '0.005em',
      'line-height': '1',
      padding: '11px 13px',
      'border-radius': '2px',
    },
  )}">${escapeHtml(label)}</a></p>`;
}

function listItem(text: string) {
  return `<li style="${inlineStyle({
    margin: '0 0 8px 0',
    color: EMAIL_COLORS.text,
    'font-size': '14px',
    'line-height': '1.5',
  })}">${escapeHtml(text)}</li>`;
}

function normalizeAppName(appName: string) {
  const trimmed = appName.trim().replace(/^the\s+/i, '');
  return `The ${trimmed}`;
}

function renderEmailDocument(input: {
  appName: string;
  headline: string;
  contentHtml: string;
  appUrl: string;
}) {
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>${escapeHtml(input.headline)}</title>
  </head>
  <body style="${inlineStyle({
    margin: '0',
    padding: '24px',
    background: EMAIL_COLORS.pageBg,
    'font-family': EMAIL_FONTS.sans,
    color: EMAIL_COLORS.text,
  })}">
    <table role="presentation" width="100%" cellspacing="0" cellpadding="0" style="${inlineStyle({ 'border-collapse': 'collapse' })}">
      <tr>
        <td align="center">
          <table role="presentation" width="100%" cellspacing="0" cellpadding="0" style="${inlineStyle(
            {
              'max-width': '640px',
              'border-collapse': 'collapse',
            },
          )}">
            <tr>
              <td style="${inlineStyle({ padding: '0 0 16px 0' })}">
                <p style="${inlineStyle({
                  margin: '0',
                  color: EMAIL_COLORS.textMuted,
                  'font-size': '12px',
                  'font-weight': '600',
                })}">${escapeHtml(input.appName)}</p>
              </td>
            </tr>
            <tr>
              <td style="${inlineStyle({
                background: EMAIL_COLORS.surface,
                border: `1px solid ${EMAIL_COLORS.border}`,
                'border-radius': '4px',
                padding: '20px',
              })}">
                <h1 style="${inlineStyle({
                  margin: '0 0 16px 0',
                  'font-family': EMAIL_FONTS.serif,
                  'font-size': '28px',
                  'font-weight': '400',
                  'line-height': '1.15',
                  'letter-spacing': '-0.01em',
                  color: EMAIL_COLORS.text,
                })}">${escapeHtml(input.headline)}</h1>
                ${input.contentHtml}
              </td>
            </tr>
            <tr>
              <td style="${inlineStyle({ padding: '14px 2px 0 2px' })}">
                <p style="${inlineStyle({
                  margin: '0',
                  color: EMAIL_COLORS.textMuted,
                  'font-size': '12px',
                  'line-height': '1.5',
                })}">
                  ${themedLink(input.appUrl, `Open ${input.appName}`)}
                </p>
              </td>
            </tr>
          </table>
        </td>
      </tr>
    </table>
  </body>
</html>`;
}

function getSettingsUrl(appUrl: string) {
  const baseUrl = appUrl.endsWith('/') ? appUrl : `${appUrl}/`;
  return new URL('settings', baseUrl).toString();
}

function withFooter(input: {
  textLines: string[];
  htmlSections: string[];
  appName: string;
  appUrl: string;
  headline: string;
}) {
  const settingsUrl = getSettingsUrl(input.appUrl);
  const contentHtml = [
    ...input.htmlSections,
    paragraph('Thanks,'),
    paragraph(`${input.appName} team`),
    `<p style="${inlineStyle({
      margin: '0',
      color: EMAIL_COLORS.textMuted,
      'font-size': '12px',
      'line-height': '1.5',
    })}">${themedLink(settingsUrl, 'Manage email preferences')}</p>`,
  ].join('');

  return {
    text: [
      ...input.textLines,
      '',
      'Thanks,',
      `${input.appName} team`,
      '',
      `Manage email preferences: ${settingsUrl}`,
    ].join('\n'),
    html: renderEmailDocument({
      appName: input.appName,
      headline: input.headline,
      appUrl: input.appUrl,
      contentHtml,
    }),
  };
}

export function renderEmailVerificationTemplate(input: {
  appName: string;
  recipientName?: string | null;
  verifyUrl: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const greeting = input.recipientName ? `Hi ${input.recipientName},` : 'Hi,';
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: 'Verify your email',
    textLines: [
      greeting,
      '',
      `Please verify your email address for ${appName} by opening this link:`,
      input.verifyUrl,
    ],
    htmlSections: [
      paragraph(greeting),
      paragraph(`Please verify your email address for ${appName}.`),
      actionButton(input.verifyUrl, 'Verify email'),
      `<p style="${inlineStyle({
        margin: '0 0 16px 0',
        color: EMAIL_COLORS.textMuted,
        'font-size': '13px',
        'line-height': '1.5',
      })}">If the button does not work, open this link: ${themedLink(input.verifyUrl, input.verifyUrl)}</p>`,
    ],
  });

  return {
    subject: `Verify your ${appName.replace('The', '').trim()} email`,
    text: footer.text,
    html: footer.html,
  };
}

export function renderPasswordResetTemplate(input: {
  appName: string;
  recipientName?: string | null;
  resetUrl: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const greeting = input.recipientName ? `Hi ${input.recipientName},` : 'Hi,';
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: 'Reset your password',
    textLines: [
      greeting,
      '',
      'A password reset was requested for your account. Use this link to set a new password:',
      input.resetUrl,
      '',
      'If you did not request this, you can safely ignore this email.',
    ],
    htmlSections: [
      paragraph(greeting),
      paragraph('A password reset was requested for your account.'),
      actionButton(input.resetUrl, 'Reset password'),
      paragraph('If you did not request this, you can safely ignore this email.'),
    ],
  });

  return {
    subject: `Reset your ${appName.replace('The', '').trim()} password`,
    text: footer.text,
    html: footer.html,
  };
}

export function renderAccountExistsTemplate(input: {
  appName: string;
  recipientName?: string | null;
  loginUrl: string;
  forgotPasswordUrl: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const greeting = input.recipientName ? `Hi ${input.recipientName},` : 'Hi,';
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: 'Someone tried to sign up with your email',
    textLines: [
      greeting,
      '',
      `Someone just tried to create a new ${appName} account using this email address, but you already have one.`,
      '',
      `If this was you, log in instead: ${input.loginUrl}`,
      `Forgot your password? Reset it here: ${input.forgotPasswordUrl}`,
      '',
      'If you did not expect this, you can safely ignore this email.',
    ],
    htmlSections: [
      paragraph(greeting),
      paragraph(
        `Someone just tried to create a new ${appName} account using this email address, but you already have one.`,
      ),
      actionButton(input.loginUrl, 'Log in'),
      `<p style="${inlineStyle({
        margin: '0 0 16px 0',
        color: EMAIL_COLORS.textMuted,
        'font-size': '13px',
        'line-height': '1.5',
      })}">Forgot your password? ${themedLink(input.forgotPasswordUrl, 'Reset it here')}.</p>`,
      paragraph('If you did not expect this, you can safely ignore this email.'),
    ],
  });

  return {
    subject: `Someone tried to sign up for ${appName.replace('The', '').trim()} with your email`,
    text: footer.text,
    html: footer.html,
  };
}

export function renderEmailInUseTemplate(input: {
  appName: string;
  recipientName?: string | null;
  forgotPasswordUrl: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const greeting = input.recipientName ? `Hi ${input.recipientName},` : 'Hi,';
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: 'This email is already associated with an account',
    textLines: [
      greeting,
      '',
      `Someone tried to change a ${appName} account's email address to this one, but it's already associated with your account.`,
      '',
      `If you forgot your password, you can reset it here: ${input.forgotPasswordUrl}`,
      '',
      'If you did not expect this, you can safely ignore this email.',
    ],
    htmlSections: [
      paragraph(greeting),
      paragraph(
        `Someone tried to change a ${appName} account's email address to this one, but it's already associated with your account.`,
      ),
      actionButton(input.forgotPasswordUrl, 'Reset password'),
      paragraph('If you did not expect this, you can safely ignore this email.'),
    ],
  });

  return {
    subject: `This email is already in use on ${appName.replace('The', '').trim()}`,
    text: footer.text,
    html: footer.html,
  };
}

export function renderPartnerInviteTemplate(input: {
  ownerName?: string | null;
  ownerEmail: string;
  appName: string;
  inviteUrl: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const inviter = input.ownerName?.trim() || input.ownerEmail;
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: `${inviter} invited you`,
    textLines: [
      `You were invited by ${inviter} to join them on ${appName}.`,
      '',
      `Open this invite link to sign in or create an account and accept: ${input.inviteUrl}`,
    ],
    htmlSections: [
      paragraph(`You were invited by ${inviter} to join them on ${appName}.`),
      actionButton(input.inviteUrl, 'Accept invitation'),
    ],
  });

  return {
    subject: `${inviter} invited you on ${appName}`,
    text: footer.text,
    html: footer.html,
  };
}

export function renderPartnerAcceptedTemplate(input: {
  partnerName?: string | null;
  partnerEmail: string;
  appName: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const partner = input.partnerName?.trim() || input.partnerEmail;
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: 'Invitation accepted',
    textLines: [
      `${partner} accepted your accountability partner invitation.`,
      '',
      `Review your dashboard here: ${input.appUrl}`,
    ],
    htmlSections: [
      paragraph(`${partner} accepted your accountability partner invitation.`),
      actionButton(input.appUrl, 'Open dashboard'),
    ],
  });

  return {
    subject: `${partner} accepted your ${appName.replace('The', '').trim()} invitation`,
    text: footer.text,
    html: footer.html,
  };
}

export function renderDeviceDeletedTemplate(input: {
  appName: string;
  appUrl: string;
  recipientName?: string | null;
  deviceName: string;
  devicePlatform: string;
  ownerName?: string | null;
  ownerEmail?: string;
  forPartner?: boolean;
}) {
  const appName = normalizeAppName(input.appName);
  const greeting = input.recipientName ? `Hi ${input.recipientName},` : 'Hi,';
  const owner = input.ownerName?.trim() || input.ownerEmail;
  const deviceLine = input.forPartner
    ? `${owner ?? 'One of your monitored accounts'} deleted the device "${input.deviceName}" (${input.devicePlatform}) from ${appName}.`
    : `Your device "${input.deviceName}" (${input.devicePlatform}) was deleted from ${appName}.`;
  const followup = input.forPartner
    ? 'If you did not expect this, review the account dashboard and recent partner activity.'
    : 'If you did not expect this change, review your account and reconnect any trusted clients.';
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: 'Device removed',
    textLines: [greeting, '', deviceLine, followup, '', `Open your dashboard: ${input.appUrl}`],
    htmlSections: [
      paragraph(greeting),
      paragraph(deviceLine),
      paragraph(followup),
      actionButton(input.appUrl, 'Open dashboard'),
    ],
  });

  return {
    subject: `Device deleted from ${appName}`,
    text: footer.text,
    html: footer.html,
  };
}

export function renderDeviceLoggedOutTemplate(input: {
  appName: string;
  appUrl: string;
  recipientName?: string | null;
  deviceName: string;
  devicePlatform: string;
  ownerName?: string | null;
  ownerEmail?: string;
  forPartner?: boolean;
}) {
  const appName = normalizeAppName(input.appName);
  const greeting = input.recipientName ? `Hi ${input.recipientName},` : 'Hi,';
  const owner = input.ownerName?.trim() || input.ownerEmail;
  const deviceLine = input.forPartner
    ? `${owner ?? 'One of your monitored accounts'} logged out of the device "${input.deviceName}" (${input.devicePlatform}), so ${appName} is no longer monitoring it.`
    : `Your device "${input.deviceName}" (${input.devicePlatform}) logged out of ${appName} and is no longer being monitored.`;
  const followup = input.forPartner
    ? 'If you did not expect this, review the account dashboard and recent activity.'
    : 'If you did not expect this, review your account and sign the device back in.';
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: 'Device logged out',
    textLines: [greeting, '', deviceLine, followup, '', `Open your dashboard: ${input.appUrl}`],
    htmlSections: [
      paragraph(greeting),
      paragraph(deviceLine),
      paragraph(followup),
      actionButton(input.appUrl, 'Open dashboard'),
    ],
  });

  return {
    subject: `Device logged out of ${appName}`,
    text: footer.text,
    html: footer.html,
  };
}

export function renderTamperAlertTemplate(input: {
  severity: TamperSeverity;
  ownerName?: string | null;
  ownerEmail: string;
  deviceName: string;
  title: string;
  details?: string | null;
  appName: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const owner = input.ownerName?.trim() || input.ownerEmail;
  const detailText = input.details?.trim();
  const deviceLine = `Device: ${input.deviceName}`;
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: `${input.severity} tamper alert`,
    textLines: [
      `${owner} triggered a ${input.severity} tamper alert.`,
      deviceLine,
      input.title,
      ...(detailText ? ['', detailText] : []),
      '',
      `Review recent screenshots and logs: ${input.appUrl}`,
    ],
    htmlSections: [
      paragraph(`${owner} triggered a ${input.severity} tamper alert.`),
      paragraph(deviceLine),
      paragraph(input.title),
      ...(detailText ? [paragraph(detailText)] : []),
      actionButton(input.appUrl, 'Review screenshots and logs'),
    ],
  });

  return {
    subject: `[${input.severity}] ${owner}: ${input.title}`,
    text: footer.text,
    html: footer.html,
  };
}

export function renderPartnerDigestTemplate(input: {
  cadence: DigestFrequency;
  partnerSummaries: Array<{
    ownerName?: string | null;
    ownerEmail: string;
    approxScreenshotCount: number;
    tamperCounts: Record<TamperSeverity, number>;
    missingLogDays: string[];
  }>;
  appName: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const periodLabel = input.cadence === 'weekly' ? 'Weekly' : 'Daily';
  const accountCount = input.partnerSummaries.length;
  const summaryTarget =
    accountCount === 1
      ? input.partnerSummaries[0]?.ownerName?.trim() || input.partnerSummaries[0]?.ownerEmail
      : `${accountCount} monitored accounts`;
  const totalTamperCounts = input.partnerSummaries.reduce<Record<TamperSeverity, number>>(
    (totals, summary) => ({
      info: totals.info + summary.tamperCounts.info,
      warning: totals.warning + summary.tamperCounts.warning,
      critical: totals.critical + summary.tamperCounts.critical,
    }),
    { info: 0, warning: 0, critical: 0 },
  );
  const totalScreenshots = input.partnerSummaries.reduce(
    (total, summary) => total + summary.approxScreenshotCount,
    0,
  );
  const missingLogHeading =
    input.cadence === 'weekly'
      ? 'Devices with at least one day without logs:'
      : 'Devices without logs in the last 24 hours:';

  const partnerSections = input.partnerSummaries.flatMap((summary) => {
    const owner = summary.ownerName?.trim() || summary.ownerEmail;
    return [
      '',
      owner,
      `Approximate screenshots available: ${summary.approxScreenshotCount}`,
      `Critical tamper alerts: ${summary.tamperCounts.critical}`,
      `Warning tamper alerts: ${summary.tamperCounts.warning}`,
      `Info-only tamper events: ${summary.tamperCounts.info}`,
      ...(summary.missingLogDays.length > 0
        ? [missingLogHeading, ...summary.missingLogDays.map((line) => `- ${line}`)]
        : []),
    ];
  });

  const lines = [
    `${periodLabel} accountability summary for ${summaryTarget}`,
    '',
    `Monitored accounts: ${accountCount}`,
    `Approximate screenshots available: ${totalScreenshots}`,
    `Critical tamper alerts: ${totalTamperCounts.critical}`,
    `Warning tamper alerts: ${totalTamperCounts.warning}`,
    `Info-only tamper events: ${totalTamperCounts.info}`,
    ...partnerSections,
    '',
    `Please review the screenshots and logs: ${input.appUrl}`,
  ];

  const summaryItems = [
    listItem(`Monitored accounts: ${accountCount}`),
    listItem(`Approximate screenshots available: ${totalScreenshots}`),
    listItem(`Critical tamper alerts: ${totalTamperCounts.critical}`),
    listItem(`Warning tamper alerts: ${totalTamperCounts.warning}`),
    listItem(`Info-only tamper events: ${totalTamperCounts.info}`),
  ].join('');

  const partnerSummarySections = input.partnerSummaries
    .map((summary) => {
      const owner = summary.ownerName?.trim() || summary.ownerEmail;
      const missingLogHtml =
        summary.missingLogDays.length > 0
          ? `<li style="${inlineStyle({
              margin: '0 0 8px 0',
              color: EMAIL_COLORS.text,
              'font-size': '15px',
              'line-height': '1.5',
            })}">${escapeHtml(missingLogHeading)}<ul style="${inlineStyle({
              margin: '8px 0 0 18px',
              padding: '0',
            })}">${summary.missingLogDays.map((line) => listItem(line)).join('')}</ul></li>`
          : '';

      return `<div style="${inlineStyle({
        margin: '0 0 16px 0',
        padding: '16px',
        border: `1px solid ${EMAIL_COLORS.border}`,
        'border-radius': '4px',
      })}">
        <p style="${inlineStyle({
          margin: '0 0 12px 0',
          color: EMAIL_COLORS.text,
          'font-size': '16px',
          'font-weight': '600',
        })}">${escapeHtml(owner)}</p>
        <ul style="${inlineStyle({
          margin: '0 0 0 18px',
          padding: '0',
        })}">
          ${listItem(`Approximate screenshots available: ${summary.approxScreenshotCount}`)}
          ${listItem(`Critical tamper alerts: ${summary.tamperCounts.critical}`)}
          ${listItem(`Warning tamper alerts: ${summary.tamperCounts.warning}`)}
          ${listItem(`Info-only tamper events: ${summary.tamperCounts.info}`)}
          ${missingLogHtml}
        </ul>
      </div>`;
    })
    .join('');

  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: `${periodLabel} summary`,
    textLines: lines,
    htmlSections: [
      paragraph(`${periodLabel} accountability summary for ${summaryTarget}.`),
      `<ul style="${inlineStyle({
        margin: '0 0 16px 18px',
        padding: '0',
      })}">${summaryItems}</ul>`,
      partnerSummarySections,
      actionButton(input.appUrl, 'Open dashboard'),
    ],
  });

  return {
    subject: `${periodLabel} summary for ${summaryTarget}`,
    text: footer.text,
    html: footer.html,
  };
}

// Internal-only notification (not sent to an app user), so this deliberately
// skips renderEmailDocument's branded card/footer and just outputs plain text
// wrapped in <pre> — there is no reader here to design for.
export function renderBugReportTemplate(input: {
  message: string;
  contactEmail?: string | null;
  reporter?: string | null;
  platform?: string | null;
  appVersion?: string | null;
  platformDetails?: string | null;
  hasLogAttachment?: boolean;
}) {
  const details: string[] = [];
  if (input.reporter) details.push(`Reporter: ${input.reporter}`);
  if (input.contactEmail) details.push(`Contact email: ${input.contactEmail}`);
  if (input.platform) details.push(`Platform: ${input.platform}`);
  if (input.appVersion) details.push(`App version: ${input.appVersion}`);
  if (input.platformDetails) details.push(`Platform details: ${input.platformDetails}`);
  if (input.hasLogAttachment) details.push('Attached: recent-logs.txt');

  const text = [...details, '', input.message].join('\n');

  return {
    subject: `Bug report: ${input.message.slice(0, 80)}`,
    text,
    html: `<pre>${escapeHtml(text)}</pre>`,
  };
}
