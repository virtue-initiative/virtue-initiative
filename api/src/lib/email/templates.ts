import { DigestFrequency, TamperSeverity } from '../email-domain';

function escapeHtml(value: string) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function paragraph(text: string) {
  return `<p>${escapeHtml(text)}</p>`;
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
}) {
  const settingsUrl = getSettingsUrl(input.appUrl);
  return {
    text: [
      ...input.textLines,
      '',
      'Thanks,',
      `The ${input.appName} team`,
      '',
      `Manage email preferences: ${settingsUrl}`,
    ].join('\n'),
    html: [
      ...input.htmlSections,
      paragraph('Thanks,'),
      paragraph(`The ${input.appName} team`),
      `<p><a href="${escapeHtml(settingsUrl)}">Manage email preferences</a></p>`,
    ].join(''),
  };
}

export function renderEmailVerificationTemplate(input: {
  appName: string;
  recipientName?: string | null;
  verifyUrl: string;
  appUrl: string;
}) {
  const greeting = input.recipientName ? `Hi ${input.recipientName},` : 'Hi,';
  const footer = withFooter({
    appName: input.appName,
    appUrl: input.appUrl,
    textLines: [
      greeting,
      '',
      `Please verify your email address for ${input.appName} by opening this link:`,
      input.verifyUrl,
    ],
    htmlSections: [
      paragraph(greeting),
      paragraph(`Please verify your email address for ${input.appName}.`),
      `<p><a href="${escapeHtml(input.verifyUrl)}">Verify email</a></p>`,
    ],
  });

  return {
    subject: `Verify your ${input.appName} email`,
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
  const greeting = input.recipientName ? `Hi ${input.recipientName},` : 'Hi,';
  const footer = withFooter({
    appName: input.appName,
    appUrl: input.appUrl,
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
      `<p><a href="${escapeHtml(input.resetUrl)}">Reset password</a></p>`,
      paragraph('If you did not request this, you can safely ignore this email.'),
    ],
  });

  return {
    subject: `Reset your ${input.appName} password`,
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
  const inviter = input.ownerName?.trim() || input.ownerEmail;
  const footer = withFooter({
    appName: input.appName,
    appUrl: input.appUrl,
    textLines: [
      `You were invited by ${inviter} to join them on ${input.appName}.`,
      '',
      `Open this invite link to sign in or create an account and accept: ${input.inviteUrl}`,
    ],
    htmlSections: [
      paragraph(`You were invited by ${inviter} to join them on ${input.appName}.`),
      `<p><a href="${escapeHtml(input.inviteUrl)}">Accept invitation</a></p>`,
    ],
  });

  return {
    subject: `${inviter} invited you on ${input.appName}`,
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
  const partner = input.partnerName?.trim() || input.partnerEmail;
  const footer = withFooter({
    appName: input.appName,
    appUrl: input.appUrl,
    textLines: [
      `${partner} accepted your accountability partner invitation.`,
      '',
      `Review your dashboard here: ${input.appUrl}`,
    ],
    htmlSections: [
      paragraph(`${partner} accepted your accountability partner invitation.`),
      `<p><a href="${escapeHtml(input.appUrl)}">Open dashboard</a></p>`,
    ],
  });

  return {
    subject: `${partner} accepted your ${input.appName} invitation`,
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
  const greeting = input.recipientName ? `Hi ${input.recipientName},` : 'Hi,';
  const owner = input.ownerName?.trim() || input.ownerEmail;
  const deviceLine = input.forPartner
    ? `${owner ?? 'One of your monitored accounts'} deleted the device "${input.deviceName}" (${input.devicePlatform}) from ${input.appName}.`
    : `Your device "${input.deviceName}" (${input.devicePlatform}) was deleted from ${input.appName}.`;
  const followup = input.forPartner
    ? 'If you did not expect this, review the account dashboard and recent partner activity.'
    : 'If you did not expect this change, review your account and reconnect any trusted clients.';
  const footer = withFooter({
    appName: input.appName,
    appUrl: input.appUrl,
    textLines: [greeting, '', deviceLine, followup, '', `Open your dashboard: ${input.appUrl}`],
    htmlSections: [
      paragraph(greeting),
      paragraph(deviceLine),
      paragraph(followup),
      `<p><a href="${escapeHtml(input.appUrl)}">Open dashboard</a></p>`,
    ],
  });

  return {
    subject: `Device deleted from ${input.appName}`,
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
  const owner = input.ownerName?.trim() || input.ownerEmail;
  const detailText = input.details?.trim();
  const footer = withFooter({
    appName: input.appName,
    appUrl: input.appUrl,
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
      `<p><a href="${escapeHtml(input.appUrl)}">Review screenshots and logs</a></p>`,
    ],
  });

  return {
    subject: `[${input.severity.toUpperCase()}] ${owner}: ${input.title}`,
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

  const footer = withFooter({
    appName: input.appName,
    appUrl: input.appUrl,
    textLines: lines,
    htmlSections: [
      paragraph(`${periodLabel} accountability summary for ${owner}.`),
      `<ul>
        <li>Approximate screenshots available: ${input.approxScreenshotCount}</li>
        <li>Critical tamper alerts: ${input.tamperCounts.critical}</li>
        <li>Warning tamper alerts: ${input.tamperCounts.warning}</li>
        <li>Info-only tamper events: ${input.tamperCounts.info}</li>
        ${
          input.missingLogDays.length > 0
            ? `<li>Devices with at least one day without logs:<ul>${input.missingLogDays
                .map((line) => `<li>${escapeHtml(line)}</li>`)
                .join('')}</ul></li>`
            : ''
        }
      </ul>`,
      paragraph('Please review the screenshots and logs.'),
      `<p><a href="${escapeHtml(input.appUrl)}">Open dashboard</a></p>`,
    ],
  });

  return {
    subject: `${periodLabel} summary for ${owner}`,
    text: footer.text,
    html: footer.html,
  };
}
