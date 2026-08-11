import { emailFrequencies, emailFrequencySchema } from '../../../shared-web/types';
import type { EmailFrequency } from '../../../shared-web/types';

export { emailFrequencies, emailFrequencySchema };
export type { EmailFrequency };

export const emailTokenPurposes = ['email_change', 'password_reset', 'partner_invite'] as const;

export type EmailTokenPurpose = (typeof emailTokenPurposes)[number];

export const tamperSeverities = ['info', 'warning', 'critical'] as const;

export type TamperSeverity = (typeof tamperSeverities)[number];

export type DigestFrequency = Extract<(typeof emailFrequencies)[number], 'daily' | 'weekly'>;

export const emailKinds = [
  'email_verification',
  'password_reset',
  'partner_invite',
  'partner_accepted',
  'device_deleted',
  'tamper_alert',
  'daily_digest',
  'weekly_digest',
  'account_exists_notice',
  'email_in_use_notice',
] as const;

export type EmailKind = (typeof emailKinds)[number];

export const EMAIL_VERIFICATION_TTL_MS = 1000 * 60 * 60 * 24;
export const PASSWORD_RESET_TTL_MS = 1000 * 60 * 60;
export const PARTNER_INVITE_TTL_MS = 1000 * 60 * 60 * 24 * 7;

export const DEFAULT_EMAIL_FREQUENCY: EmailFrequency = 'daily';
