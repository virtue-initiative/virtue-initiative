import {
  DEFAULT_EMAIL_FREQUENCY,
  DEFAULT_IMMEDIATE_TAMPER_SEVERITY,
  TamperSeverity,
  severityAtLeast,
} from './email-domain';
import { findUserById, listAcceptedNotificationTargetsForUser } from './db';
import { sendEmail } from './email';
import { renderTamperAlertTemplate } from './email/templates';
import { Env } from '../types/bindings';
import { getRiskRating, type RiskRating } from '../../../shared-web/risk';

const riskRatingToSeverity: Record<RiskRating, TamperSeverity> = {
  marginal: 'info',
  moderate: 'warning',
  high: 'critical',
};

export function riskToSeverity(risk: number | null | undefined): TamperSeverity | null {
  const rating = getRiskRating(risk);
  return rating ? riskRatingToSeverity[rating] : null;
}

export async function notifyPartnersAboutRiskLog(
  db: D1Database,
  env: Env,
  input: {
    logId: string;
    appUrl?: string;
    userId: string;
    deviceName: string;
    severity: TamperSeverity;
    risk: number;
    title: string;
    details?: string | null;
    happenedAt: number;
  },
) {
  const owner = await findUserById(db, input.userId);
  if (!owner) {
    return;
  }

  const targets = await listAcceptedNotificationTargetsForUser(db, input.userId);
  for (const target of targets) {
    if ((target.email_frequency ?? DEFAULT_EMAIL_FREQUENCY) === 'none') {
      continue;
    }

    if (!severityAtLeast(input.severity, DEFAULT_IMMEDIATE_TAMPER_SEVERITY)) {
      continue;
    }

    const email = renderTamperAlertTemplate({
      appName: env.APP_NAME,
      severity: input.severity,
      ownerName: owner.name,
      ownerEmail: owner.email,
      deviceName: input.deviceName,
      title: input.title,
      details: input.details ?? null,
      appUrl: input.appUrl ?? env.APP_URL,
    });

    await sendEmail({
      env,
      db,
      kind: 'tamper_alert',
      recipient: target.watcher_email,
      subject: email.subject,
      text: email.text,
      html: email.html,
      related_user_id: input.userId,
      related_partnership_id: target.partnership_id,
      metadata: {
        email_frequency: target.email_frequency ?? DEFAULT_EMAIL_FREQUENCY,
        logId: input.logId,
        deviceName: input.deviceName,
        risk: input.risk,
        severity: input.severity,
        happenedAt: input.happenedAt,
      },
    });
  }
}
