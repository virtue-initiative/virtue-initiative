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
  danger: '#9b2c2c',
  dangerBg: '#f6e3df',
  warning: '#7a5200',
  warningBg: '#f5ebd0',
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

export const HELP_EMAIL = 'help@virtueinitiative.org';
const HELP_LINE = `If you need help, reach out to ${HELP_EMAIL}.`;

const ENCOURAGEMENT_NOTE =
  'Remember, the goal is not to bring shame, but to bring encouragement to live with integrity.';

const DIGEST_NOTE =
  'If you need to reach out to your partner, remember the goal is not to bring shame, but to bring encouragement to live with integrity.';

const ALERT_NOTE = `This email was sent because our automated checks found something possibly questionable. Please review the activity and talk with your partner. ${ENCOURAGEMENT_NOTE}`;

function paragraph(text: string) {
  return `<p style="${inlineStyle({
    margin: '0 0 16px 0',
    color: EMAIL_COLORS.text,
    'font-size': '16px',
    'line-height': '1.5',
  })}">${escapeHtml(text)}</p>`;
}

// Mirrors .vi-btn--primary in shared-web/components/Button/Button.css.
// `compact` is a smaller button for the repeated links inside a list of sections.
function actionButton(url: string, label: string, options: { compact?: boolean } = {}) {
  const compact = options.compact ?? false;
  return `<p style="${inlineStyle({ margin: compact ? '0' : '0 0 16px 0' })}"><a href="${escapeHtml(url)}" style="${inlineStyle(
    {
      display: 'inline-block',
      background: EMAIL_COLORS.accent,
      color: EMAIL_COLORS.textOnAccent,
      'text-decoration': 'none',
      'font-weight': '500',
      'font-size': compact ? '13px' : '14px',
      'letter-spacing': '0.005em',
      'line-height': '1',
      padding: compact ? '9px 12px' : '11px 13px',
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
    `<p style="${inlineStyle({
      margin: '8px 0 0 0',
      padding: '16px 0 0 0',
      'border-top': `1px solid ${EMAIL_COLORS.border}`,
      color: EMAIL_COLORS.textMuted,
      'font-size': '12px',
      'line-height': '1.6',
    })}">Thanks,<br />${escapeHtml(input.appName)} team<br />${themedLink(settingsUrl, 'Manage email preferences')}</p>`,
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

export function renderPasswordChangedTemplate(input: {
  appName: string;
  recipientName?: string | null;
  forgotPasswordUrl: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const greeting = input.recipientName ? `Hi ${input.recipientName},` : 'Hi,';
  const summary =
    'The password for your account was just changed. Any other browsers signed in to your account were logged out. Your devices are still being monitored.';
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: 'Your password was changed',
    textLines: [
      greeting,
      '',
      summary,
      '',
      `If you did not do this, reset your password now: ${input.forgotPasswordUrl}`,
      HELP_LINE,
    ],
    htmlSections: [
      paragraph(greeting),
      paragraph(summary),
      paragraph(`If you did not do this, reset your password now. ${HELP_LINE}`),
      actionButton(input.forgotPasswordUrl, 'Reset password'),
    ],
  });

  return {
    subject: `Your ${appName.replace('The', '').trim()} password was changed`,
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
      `If you did not expect this, you can safely ignore this email. ${HELP_LINE}`,
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
      paragraph(`If you did not expect this, you can safely ignore this email. ${HELP_LINE}`),
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
      `If you did not expect this, you can safely ignore this email. ${HELP_LINE}`,
    ],
    htmlSections: [
      paragraph(greeting),
      paragraph(
        `Someone tried to change a ${appName} account's email address to this one, but it's already associated with your account.`,
      ),
      actionButton(input.forgotPasswordUrl, 'Reset password'),
      paragraph(`If you did not expect this, you can safely ignore this email. ${HELP_LINE}`),
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
      `You were invited by ${inviter} to be their accountability partner on ${appName}.`,
      '',
      `Open this invite link to sign in or create an account and accept: ${input.inviteUrl}`,
    ],
    htmlSections: [
      paragraph(
        `You were invited by ${inviter} to be their accountability partner on ${appName}. If you accept, you will be able to review their activity on their devices.`,
      ),
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

export function renderPartnerRemovedTemplate(input: {
  recipientName?: string | null;
  otherName?: string | null;
  otherEmail: string;
  /** True when the recipient is the monitored account that the partner left. */
  forOwner: boolean;
  appName: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const greeting = input.recipientName ? `Hi ${input.recipientName},` : 'Hi,';
  const other = input.otherName?.trim() || input.otherEmail;
  const summary = input.forOwner
    ? `${other} is no longer your accountability partner on ${appName}. They will no longer receive your alerts or reports.`
    : `${other} removed you as their accountability partner on ${appName}. You will no longer receive their alerts or reports.`;
  const followup = input.forOwner
    ? `Reach out to ${other} if this was unexpected. ${HELP_LINE}`
    : `Reach out to ${other} if you have questions.`;
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: 'Partnership ended',
    textLines: [greeting, '', summary, followup, '', `Open your dashboard: ${input.appUrl}`],
    htmlSections: [
      paragraph(greeting),
      paragraph(summary),
      paragraph(followup),
      actionButton(input.appUrl, 'Open dashboard'),
    ],
  });

  return {
    subject: `${other} and you are no longer accountability partners`,
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
    ? `Reach out to ${owner ?? 'the owner of the device'} if this was unexpected.`
    : `If this was unexpected, check your account activity. ${HELP_LINE}`;
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
    ? `Reach out to ${owner ?? 'the owner of the device'} if this was unexpected.`
    : `If this was unexpected, check your account activity. ${HELP_LINE}`;
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

const alertSeverityLabels: Record<TamperSeverity, string> = {
  critical: 'High risk',
  warning: 'Medium risk',
  info: 'Low risk',
};

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
  const alertLabel = `${alertSeverityLabels[input.severity]} alert`;
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: alertLabel,
    textLines: [
      `${owner} has a new ${alertLabel.toLowerCase()}.`,
      deviceLine,
      input.title,
      ...(detailText ? ['', detailText] : []),
      '',
      ALERT_NOTE,
      '',
      `Review recent screenshots and logs: ${input.appUrl}`,
    ],
    htmlSections: [
      paragraph(`${owner} has a new ${alertLabel.toLowerCase()}.`),
      paragraph(deviceLine),
      paragraph(input.title),
      ...(detailText ? [paragraph(detailText)] : []),
      paragraph(ALERT_NOTE),
      actionButton(input.appUrl, 'Review screenshots and logs'),
    ],
  });

  return {
    subject: `${alertLabel}: ${owner}: ${input.title}`,
    text: footer.text,
    html: footer.html,
  };
}

export function renderLockedPasswordAccessedTemplate(input: {
  ownerName?: string | null;
  ownerEmail: string;
  label: string;
  appName: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const owner = input.ownerName?.trim() || input.ownerEmail;
  const line = `${owner} just accessed a locked password labeled "${input.label}". If this was not expected, you should ask them about it. Locked passwords are designed to stay hidden from the owner unless they really need them.`;
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: 'Locked password accessed',
    textLines: [line, '', ENCOURAGEMENT_NOTE, '', `Review the account: ${input.appUrl}`],
    htmlSections: [
      paragraph(line),
      paragraph(ENCOURAGEMENT_NOTE),
      actionButton(input.appUrl, 'Review account'),
    ],
  });

  return {
    subject: `${owner} accessed a locked password`,
    text: footer.text,
    html: footer.html,
  };
}

type StatTone = 'danger' | 'warning';

// A row of equal tiles, each a big number over a label. A toned tile is tinted
// only when its number is above zero, so an all-clear report stays quiet and a
// problem is the first thing the eye lands on.
function statRow(stats: Array<{ value: number; label: string; tone?: StatTone }>) {
  const cells = stats
    .map(({ value, label, tone }) => {
      const active = tone !== undefined && value > 0;
      const color = !active
        ? EMAIL_COLORS.text
        : tone === 'danger'
          ? EMAIL_COLORS.danger
          : EMAIL_COLORS.warning;
      const background = !active
        ? EMAIL_COLORS.pageBg
        : tone === 'danger'
          ? EMAIL_COLORS.dangerBg
          : EMAIL_COLORS.warningBg;
      return `<td width="${Math.floor(100 / stats.length)}%" style="${inlineStyle({
        background,
        'border-radius': '4px',
        padding: '12px 14px',
        'vertical-align': 'top',
      })}">
        <div style="${inlineStyle({
          'font-family': EMAIL_FONTS.serif,
          'font-size': '28px',
          'line-height': '1.1',
          color,
        })}">${escapeHtml(value.toLocaleString('en-US'))}</div>
        <div style="${inlineStyle({
          'margin-top': '4px',
          'font-size': '12px',
          'line-height': '1.3',
          color: active ? color : EMAIL_COLORS.textMuted,
        })}">${escapeHtml(label)}</div>
      </td>`;
    })
    .join('');

  return `<table role="presentation" width="100%" cellspacing="0" cellpadding="0" style="${inlineStyle(
    {
      'border-collapse': 'separate',
      'border-spacing': '6px 0',
      margin: '0 -6px 16px -6px',
      width: 'calc(100% + 12px)',
    },
  )}"><tr>${cells}</tr></table>`;
}

function missingLogsCallout(heading: string, entries: Array<{ name: string; detail?: string }>) {
  const items = entries
    .map(
      ({ name, detail }) =>
        `<li style="${inlineStyle({
          margin: '0 0 4px 0',
          color: EMAIL_COLORS.text,
          'font-size': '14px',
          'line-height': '1.5',
        })}">${escapeHtml(name)}${
          detail
            ? ` <span style="${inlineStyle({ color: EMAIL_COLORS.textMuted })}">${escapeHtml(detail)}</span>`
            : ''
        }</li>`,
    )
    .join('');

  return `<div style="${inlineStyle({
    margin: '0 0 16px 0',
    padding: '12px 14px',
    background: EMAIL_COLORS.warningBg,
    'border-left': `3px solid ${EMAIL_COLORS.warning}`,
    'border-radius': '2px',
  })}">
    <p style="${inlineStyle({
      margin: '0 0 8px 0',
      color: EMAIL_COLORS.warning,
      'font-size': '13px',
      'font-weight': '600',
      'line-height': '1.4',
    })}">${escapeHtml(heading)}</p>
    <ul style="${inlineStyle({ margin: '0', padding: '0 0 0 18px' })}">${items}</ul>
  </div>`;
}

export function renderPartnerDigestTemplate(input: {
  cadence: DigestFrequency;
  partnerSummaries: Array<{
    ownerName?: string | null;
    ownerEmail: string;
    approxScreenshotCount: number;
    tamperCounts: Record<TamperSeverity, number>;
    /**
     * Devices with no logs in the period. `days` lists the days missing, for a weekly digest;
     * it is empty for a daily one, where the whole period is missing.
     */
    missingLogs: Array<{ deviceName: string; days: string[] }>;
    /** Web app report covering this digest's day or week for this person. */
    reportUrl: string;
  }>;
  appName: string;
  appUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const periodLabel = input.cadence === 'weekly' ? 'Weekly' : 'Daily';
  const accountCount = input.partnerSummaries.length;
  const ownerLabel = (summary: (typeof input.partnerSummaries)[number]) =>
    summary.ownerName?.trim() || summary.ownerEmail;
  const summaryTarget =
    accountCount === 1
      ? ownerLabel(input.partnerSummaries[0]!)
      : `${accountCount} monitored accounts`;
  const missingLogHeading =
    input.cadence === 'weekly'
      ? 'Devices with at least one day without logs:'
      : 'Devices without logs in the last 24 hours:';
  const missingLogLine = (log: { deviceName: string; days: string[] }) =>
    log.days.length > 0
      ? `${log.deviceName}: no logs on ${log.days.join(', ')}`
      : `${log.deviceName}: no logs in the last 24 hours`;

  const partnerSections = input.partnerSummaries.flatMap((summary) => {
    const owner = ownerLabel(summary);
    return [
      '',
      owner,
      `Approximate screenshots available: ${summary.approxScreenshotCount}`,
      `High risk alerts and screenshots: ${summary.tamperCounts.critical}`,
      `Medium risk alerts and screenshots: ${summary.tamperCounts.warning}`,
      ...(summary.missingLogs.length > 0
        ? [missingLogHeading, ...summary.missingLogs.map((log) => `- ${missingLogLine(log)}`)]
        : []),
      `Read ${owner}'s report: ${summary.reportUrl}`,
    ];
  });

  // One person: a single call to action. Several: each section links its own report.
  const singleSummary = accountCount === 1 ? input.partnerSummaries[0] : undefined;

  const lines = [
    `${periodLabel} accountability summary for ${summaryTarget}`,
    ...partnerSections,
    '',
    DIGEST_NOTE,
  ];

  const personBlock = (summary: (typeof input.partnerSummaries)[number], index: number) => {
    const owner = ownerLabel(summary);
    return `<div style="${inlineStyle({
      margin: '0 0 16px 0',
      padding: index === 0 ? '0' : '16px 0 0 0',
      'border-top': index === 0 ? 'none' : `1px solid ${EMAIL_COLORS.border}`,
    })}">
      <p style="${inlineStyle({
        margin: '0 0 10px 0',
        color: EMAIL_COLORS.text,
        'font-size': '16px',
        'font-weight': '600',
      })}">${escapeHtml(owner)}</p>
      ${personStats(summary)}
      ${actionButton(summary.reportUrl, `Read ${owner}'s report`, { compact: true })}
    </div>`;
  };

  const personStats = (summary: (typeof input.partnerSummaries)[number]) =>
    statRow([
      { value: summary.approxScreenshotCount, label: 'Screenshots' },
      { value: summary.tamperCounts.critical, label: 'High risk', tone: 'danger' },
      { value: summary.tamperCounts.warning, label: 'Medium risk', tone: 'warning' },
    ]) +
    (summary.missingLogs.length > 0
      ? missingLogsCallout(
          missingLogHeading,
          summary.missingLogs.map((log) => ({
            name: log.deviceName,
            detail: log.days.length > 0 ? log.days.join(', ') : undefined,
          })),
        )
      : '');

  // One person's numbers sit directly under the intro. Several people each get a section.
  // Counts are never summed across people.
  const bodySections = singleSummary
    ? [personStats(singleSummary)]
    : [input.partnerSummaries.map(personBlock).join('')];

  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: `${periodLabel} summary`,
    textLines: lines,
    htmlSections: [
      paragraph(`${periodLabel} accountability summary for ${summaryTarget}.`),
      ...bodySections,
      paragraph(DIGEST_NOTE),
      ...(singleSummary
        ? [actionButton(singleSummary.reportUrl, `Read ${ownerLabel(singleSummary)}'s report`)]
        : []),
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
    subject: `Issue report: ${input.message.slice(0, 80)}`,
    text,
    html: `<pre>${escapeHtml(text)}</pre>`,
  };
}

// Sent to the reporter. The address may be an unverified contact_email typed
// into an anonymous form, so this deliberately echoes none of the report's
// text (API-042); otherwise the endpoint would relay arbitrary content.
export function renderBugReportConfirmationTemplate(input: { appName: string; appUrl: string }) {
  const appName = normalizeAppName(input.appName);
  const received = 'We received your issue report.';
  const addMore = 'Reply to this email to add details or screenshots.';
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: 'We received your issue report',
    textLines: ['Hi,', '', received, '', addMore],
    htmlSections: [paragraph('Hi,'), paragraph(received), paragraph(addMore)],
  });

  return {
    subject: 'We received your issue report',
    text: footer.text,
    html: footer.html,
  };
}

// Sent to an address typed into the landing site's newsletter form (API-058).
export function renderNewsletterConfirmationTemplate(input: {
  appName: string;
  appUrl: string;
  confirmUrl: string;
}) {
  const appName = normalizeAppName(input.appName);
  const asked = `Someone, hopefully you, asked to get ${appName} project updates at this address.`;
  const ignore = "If this wasn't you, ignore this email and you won't hear from us again.";
  const footer = withFooter({
    appName,
    appUrl: input.appUrl,
    headline: 'Confirm your subscription',
    textLines: [asked, '', `Confirm your subscription: ${input.confirmUrl}`, '', ignore],
    htmlSections: [
      paragraph(asked),
      actionButton(input.confirmUrl, 'Confirm subscription'),
      paragraph(ignore),
    ],
  });

  return {
    subject: `Confirm your ${appName} newsletter subscription`,
    text: footer.text,
    html: footer.html,
  };
}
