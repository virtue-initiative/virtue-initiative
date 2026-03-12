export const emailTokenPurposes = ['email_verification', 'password_reset'] as const;

export type EmailTokenPurpose = (typeof emailTokenPurposes)[number];

export const tamperSeverities = ['info', 'warning', 'critical'] as const;
export const immediateTamperSeverities = ['warning', 'critical'] as const;

export type TamperSeverity = (typeof tamperSeverities)[number];
export type ImmediateTamperSeverity = (typeof immediateTamperSeverities)[number];

export const digestCadences = ['none', 'daily', 'twice_weekly', 'weekly'] as const;

export type DigestCadence = (typeof digestCadences)[number];

export const emailKinds = [
  'email_verification',
  'password_reset',
  'partner_invite',
  'partner_accepted',
  'device_deleted',
  'tamper_alert',
  'daily_digest',
  'twice_weekly_digest',
  'weekly_digest',
] as const;

export type EmailKind = (typeof emailKinds)[number];

export const EMAIL_VERIFICATION_TTL_MS = 1000 * 60 * 60 * 24;
export const PASSWORD_RESET_TTL_MS = 1000 * 60 * 60;
export const PARTNER_INVITE_TTL_MS = 1000 * 60 * 60 * 24 * 7;

export const DEFAULT_DIGEST_CADENCE: DigestCadence = 'daily';
export const DEFAULT_IMMEDIATE_TAMPER_SEVERITY: TamperSeverity = 'critical';

export function normalizeImmediateTamperSeverity(
  value: string | null | undefined,
): ImmediateTamperSeverity {
  return value === 'warning' ? 'warning' : 'critical';
}

const severityRankings: Record<TamperSeverity, number> = {
  info: 0,
  warning: 1,
  critical: 2,
};

export function severityAtLeast(value: TamperSeverity, minimum: TamperSeverity) {
  return severityRankings[value] >= severityRankings[minimum];
}
