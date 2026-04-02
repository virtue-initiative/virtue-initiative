import { DigestFrequency, TamperSeverity } from '../email-domain';

const EMAIL_COLORS = {
  text: '#1a1a1a',
  textMuted: '#6b6860',
  textOnAccent: '#ffffff',
  accent: '#008900',
  pageBg: '#f9f9f7',
  surface: '#ffffff',
  border: '#e0ddd8',
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
    'line-height': '1.6',
  })}">${escapeHtml(text)}</p>`;
}

function actionButton(url: string, label: string) {
  return `<p style="${inlineStyle({ margin: '0 0 18px 0' })}"><a href="${escapeHtml(url)}" style="${inlineStyle(
    {
      display: 'inline-block',
      background: EMAIL_COLORS.accent,
      color: EMAIL_COLORS.textOnAccent,
      'text-decoration': 'none',
      'font-weight': '600',
      'font-size': '15px',
      'line-height': '1',
      padding: '12px 18px',
      'border-radius': '8px',
    },
  )}">${escapeHtml(label)}</a></p>`;
}

function listItem(text: string) {
  return `<li style="${inlineStyle({
    margin: '0 0 8px 0',
    color: EMAIL_COLORS.text,
    'font-size': '15px',
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
    'font-family': "-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif",
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
              <td style="${inlineStyle({ padding: '0 0 14px 0' })}">
                <p style="${inlineStyle({
                  margin: '0',
                  color: EMAIL_COLORS.textMuted,
                  'font-size': '13px',
                  'font-weight': '600',
                })}">${escapeHtml(input.appName)}</p>
              </td>
            </tr>
            <tr>
              <td style="${inlineStyle({
                background: EMAIL_COLORS.surface,
                border: `1px solid ${EMAIL_COLORS.border}`,
                'border-radius': '14px',
                padding: '26px 24px',
              })}">
                <h1 style="${inlineStyle({
                  margin: '0 0 16px 0',
                  'font-size': '22px',
                  'line-height': '1.25',
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
      'font-size': '13px',
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

export function renderTamperAlertTemplate(input: {
  severity: TamperSeverity;
  ownerName?: string | null;
  ownerEmail: string;
  title: string;
  details?: string | null;
  appName: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const owner = input.ownerName?.trim() || input.ownerEmail;
  const detailText = input.details?.trim();
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: `${input.severity} tamper alert`,
    textLines: [
      `${owner} triggered a ${input.severity} tamper alert.`,
      input.title,
      ...(detailText ? ['', detailText] : []),
      '',
      `Review recent screenshots and logs: ${input.appUrl}`,
    ],
    htmlSections: [
      paragraph(`${owner} triggered a ${input.severity} tamper alert.`),
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
  ownerName?: string | null;
  ownerEmail: string;
  approxScreenshotCount: number;
  tamperCounts: Record<TamperSeverity, number>;
  missingLogDays: string[];
  appName: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const owner = input.ownerName?.trim() || input.ownerEmail;
  const periodLabel = input.cadence === 'weekly' ? 'Weekly' : 'Daily';
  const lines = [
    `${periodLabel} accountability summary for ${owner}`,
    '',
    `Approximate screenshots available: ${input.approxScreenshotCount}`,
    `Critical tamper alerts: ${input.tamperCounts.critical}`,
    `Warning tamper alerts: ${input.tamperCounts.warning}`,
    `Info-only tamper events: ${input.tamperCounts.info}`,
    ...(input.missingLogDays.length > 0
      ? [
          '',
          'Devices with at least one day without logs:',
          ...input.missingLogDays.map((line) => `- ${line}`),
        ]
      : []),
    '',
    `Please review the screenshots and logs: ${input.appUrl}`,
  ];

  const summaryItems = [
    listItem(`Approximate screenshots available: ${input.approxScreenshotCount}`),
    listItem(`Critical tamper alerts: ${input.tamperCounts.critical}`),
    listItem(`Warning tamper alerts: ${input.tamperCounts.warning}`),
    listItem(`Info-only tamper events: ${input.tamperCounts.info}`),
    ...(input.missingLogDays.length > 0
      ? [
          `<li style="${inlineStyle({
            margin: '0 0 8px 0',
            color: EMAIL_COLORS.text,
            'font-size': '15px',
            'line-height': '1.5',
          })}">Devices with at least one day without logs:<ul style="${inlineStyle({
            margin: '8px 0 0 18px',
            padding: '0',
          })}">${input.missingLogDays.map((line) => listItem(line)).join('')}</ul></li>`,
        ]
      : []),
  ].join('');

  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: `${periodLabel} summary`,
    textLines: lines,
    htmlSections: [
      paragraph(`${periodLabel} accountability summary for ${owner}.`),
      `<ul style="${inlineStyle({
        margin: '0 0 16px 18px',
        padding: '0',
      })}">${summaryItems}</ul>`,
      actionButton(input.appUrl, 'Open dashboard'),
    ],
  });

  return {
    subject: `${periodLabel} summary for ${owner}`,
    text: footer.text,
    html: footer.html,
  };
}
