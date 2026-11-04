import {
  renderAccountExistsTemplate,
  renderBugReportConfirmationTemplate,
  renderBugReportTemplate,
  renderDeviceDeletedTemplate,
  renderDeviceLoggedOutTemplate,
  renderEmailInUseTemplate,
  renderEmailVerificationTemplate,
  renderLockedPasswordAccessedTemplate,
  renderNewsletterConfirmationTemplate,
  renderPartnerAcceptedTemplate,
  renderPartnerDigestTemplate,
  renderPartnerInviteTemplate,
  renderPartnerRemovedTemplate,
  renderPasswordChangedTemplate,
  renderPasswordResetTemplate,
  renderTamperAlertTemplate,
} from './templates';

export interface EmailSample {
  id: string;
  label: string;
  /** Who the real email would go to, shown in the preview's header. */
  to: string;
  email: { subject: string; text: string; html: string };
}

type DigestSummary = Parameters<typeof renderPartnerDigestTemplate>[0]['partnerSummaries'][number];

/**
 * One rendering of every email template with made-up data, for the dev preview (API-066).
 * Several templates appear more than once where their shape changes with the input.
 */
export function buildEmailSamples(env: { APP_NAME: string; APP_URL: string }): EmailSample[] {
  const { APP_NAME: appName, APP_URL: appUrl } = env;
  const viewer = 'jordan@example.com';
  const link = (path: string) => `${appUrl}${path}`;

  const reportUrl = (id: string) => link(`/report/${id}?date=2026-11-03`);
  const quiet: DigestSummary = {
    ownerName: 'Sam Rivera',
    ownerEmail: 'sam@example.com',
    approxScreenshotCount: 214,
    tamperCounts: { info: 0, warning: 0, critical: 0 },
    missingLogs: [],
    reportUrl: reportUrl('sam'),
  };
  const flagged: DigestSummary = {
    ownerName: 'Alex Kim',
    ownerEmail: 'alex@example.com',
    approxScreenshotCount: 1342,
    tamperCounts: { info: 0, warning: 4, critical: 2 },
    missingLogs: [{ deviceName: 'Pixel 8', days: [] }],
    reportUrl: reportUrl('alex'),
  };
  const manyDevices: DigestSummary = {
    ownerName: 'Test 1',
    ownerEmail: 'test1@example.com',
    approxScreenshotCount: 367,
    tamperCounts: { info: 0, warning: 0, critical: 0 },
    missingLogs: [
      'Google sdk_gphone64_x86_64',
      'iPhone 16',
      'Iphone release 0.1.7',
      'Android 0.1.7test1@virtueinitiative.org',
      'Android 0.1.7',
      'red',
      'red',
    ].map((deviceName) => ({ deviceName, days: [] })),
    reportUrl: reportUrl('test1'),
  };

  return [
    {
      id: 'digest-daily-single',
      label: 'Daily digest, one account',
      to: viewer,
      email: renderPartnerDigestTemplate({
        cadence: 'daily',
        partnerSummaries: [flagged],
        appName,
        appUrl,
      }),
    },
    {
      id: 'digest-daily-single-quiet',
      label: 'Daily digest, all clear',
      to: viewer,
      email: renderPartnerDigestTemplate({
        cadence: 'daily',
        partnerSummaries: [quiet],
        appName,
        appUrl,
      }),
    },
    {
      id: 'digest-daily-many-devices',
      label: 'Daily digest, many silent devices',
      to: viewer,
      email: renderPartnerDigestTemplate({
        cadence: 'daily',
        partnerSummaries: [manyDevices],
        appName,
        appUrl,
      }),
    },
    {
      id: 'digest-daily-multiple',
      label: 'Daily digest, several accounts',
      to: viewer,
      email: renderPartnerDigestTemplate({
        cadence: 'daily',
        partnerSummaries: [flagged, quiet, manyDevices],
        appName,
        appUrl,
      }),
    },
    {
      id: 'digest-weekly',
      label: 'Weekly digest',
      to: viewer,
      email: renderPartnerDigestTemplate({
        cadence: 'weekly',
        partnerSummaries: [
          {
            ...flagged,
            missingLogs: [
              { deviceName: 'Pixel 8', days: ['2026-10-29', '2026-10-30', '2026-11-02'] },
              { deviceName: 'Work laptop', days: ['2026-11-01'] },
            ],
          },
          quiet,
        ],
        appName,
        appUrl,
      }),
    },
    {
      id: 'alert-high',
      label: 'Tamper alert, high risk',
      to: viewer,
      email: renderTamperAlertTemplate({
        severity: 'critical',
        ownerName: 'Alex Kim',
        ownerEmail: 'alex@example.com',
        deviceName: 'Alex’s laptop',
        title: 'Monitoring was uninstalled',
        details: 'The Virtue Initiative service was removed from this device at 2:14 AM.',
        appName,
        appUrl,
      }),
    },
    {
      id: 'alert-medium',
      label: 'Tamper alert, medium risk',
      to: viewer,
      email: renderTamperAlertTemplate({
        severity: 'warning',
        ownerName: null,
        ownerEmail: 'alex@example.com',
        deviceName: 'Pixel 8',
        title: 'Screenshots stopped uploading',
        appName,
        appUrl,
      }),
    },
    {
      id: 'locked-password-accessed',
      label: 'Locked password accessed',
      to: viewer,
      email: renderLockedPasswordAccessedTemplate({
        ownerName: 'Alex Kim',
        ownerEmail: 'alex@example.com',
        label: 'Streaming account',
        appName,
        appUrl,
      }),
    },
    {
      id: 'device-deleted-owner',
      label: 'Device deleted, to owner',
      to: 'alex@example.com',
      email: renderDeviceDeletedTemplate({
        appName,
        appUrl,
        recipientName: 'Alex',
        deviceName: 'Pixel 8',
        devicePlatform: 'Android',
      }),
    },
    {
      id: 'device-deleted-partner',
      label: 'Device deleted, to partner',
      to: viewer,
      email: renderDeviceDeletedTemplate({
        appName,
        appUrl,
        recipientName: 'Jordan',
        deviceName: 'Pixel 8',
        devicePlatform: 'Android',
        ownerName: 'Alex Kim',
        ownerEmail: 'alex@example.com',
        forPartner: true,
      }),
    },
    {
      id: 'device-logged-out-owner',
      label: 'Device logged out, to owner',
      to: 'alex@example.com',
      email: renderDeviceLoggedOutTemplate({
        appName,
        appUrl,
        recipientName: 'Alex',
        deviceName: 'Alex’s laptop',
        devicePlatform: 'Windows',
      }),
    },
    {
      id: 'device-logged-out-partner',
      label: 'Device logged out, to partner',
      to: viewer,
      email: renderDeviceLoggedOutTemplate({
        appName,
        appUrl,
        recipientName: 'Jordan',
        deviceName: 'Alex’s laptop',
        devicePlatform: 'Windows',
        ownerName: 'Alex Kim',
        ownerEmail: 'alex@example.com',
        forPartner: true,
      }),
    },
    {
      id: 'partner-invite',
      label: 'Partner invitation',
      to: viewer,
      email: renderPartnerInviteTemplate({
        ownerName: 'Alex Kim',
        ownerEmail: 'alex@example.com',
        appName,
        inviteUrl: link('/invite/abc123'),
        appUrl,
      }),
    },
    {
      id: 'partner-accepted',
      label: 'Partner accepted',
      to: 'alex@example.com',
      email: renderPartnerAcceptedTemplate({
        partnerName: 'Jordan Lee',
        partnerEmail: viewer,
        appName,
        appUrl,
      }),
    },
    {
      id: 'partner-removed-owner',
      label: 'Partnership ended, to owner',
      to: 'alex@example.com',
      email: renderPartnerRemovedTemplate({
        recipientName: 'Alex',
        otherName: 'Jordan Lee',
        otherEmail: viewer,
        forOwner: true,
        appName,
        appUrl,
      }),
    },
    {
      id: 'partner-removed-partner',
      label: 'Partnership ended, to partner',
      to: viewer,
      email: renderPartnerRemovedTemplate({
        recipientName: 'Jordan',
        otherName: 'Alex Kim',
        otherEmail: 'alex@example.com',
        forOwner: false,
        appName,
        appUrl,
      }),
    },
    {
      id: 'email-verification',
      label: 'Verify email',
      to: viewer,
      email: renderEmailVerificationTemplate({
        appName,
        recipientName: 'Jordan',
        verifyUrl: link('/verify-email?token=abc123'),
        appUrl,
      }),
    },
    {
      id: 'password-reset',
      label: 'Reset password',
      to: viewer,
      email: renderPasswordResetTemplate({
        appName,
        recipientName: 'Jordan',
        resetUrl: link('/reset-password?token=abc123'),
        appUrl,
      }),
    },
    {
      id: 'password-changed',
      label: 'Password changed',
      to: viewer,
      email: renderPasswordChangedTemplate({
        appName,
        recipientName: 'Jordan',
        forgotPasswordUrl: link('/forgot-password'),
        appUrl,
      }),
    },
    {
      id: 'account-exists',
      label: 'Sign-up for existing account',
      to: viewer,
      email: renderAccountExistsTemplate({
        appName,
        recipientName: 'Jordan',
        loginUrl: link('/login'),
        forgotPasswordUrl: link('/forgot-password'),
        appUrl,
      }),
    },
    {
      id: 'email-in-use',
      label: 'Email change to address in use',
      to: viewer,
      email: renderEmailInUseTemplate({
        appName,
        recipientName: 'Jordan',
        forgotPasswordUrl: link('/forgot-password'),
        appUrl,
      }),
    },
    {
      id: 'newsletter-confirmation',
      label: 'Newsletter confirmation',
      to: viewer,
      email: renderNewsletterConfirmationTemplate({
        appName,
        appUrl,
        confirmUrl: link('/newsletter/confirm?token=abc123'),
      }),
    },
    {
      id: 'bug-report-confirmation',
      label: 'Issue report received',
      to: viewer,
      email: renderBugReportConfirmationTemplate({ appName, appUrl }),
    },
    {
      id: 'bug-report',
      label: 'Issue report, internal',
      to: 'help@virtueinitiative.org',
      email: renderBugReportTemplate({
        message: 'The app crashes when I open settings.',
        contactEmail: viewer,
        reporter: 'Jordan Lee',
        platform: 'android',
        appVersion: '0.1.7',
        hasLogAttachment: true,
      }),
    },
  ];
}
