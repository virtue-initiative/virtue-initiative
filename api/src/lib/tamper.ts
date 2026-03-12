import {
  DEFAULT_DIGEST_CADENCE,
  DEFAULT_IMMEDIATE_TAMPER_SEVERITY,
  normalizeImmediateTamperSeverity,
  TamperSeverity,
  severityAtLeast,
} from './email-domain';
import { findUserById, listAcceptedNotificationTargetsForUser } from './db';
import { sendEmail } from './email';
import { renderTamperAlertTemplate } from './email/templates';
import { Env } from '../types/bindings';

export interface ClassifiedRiskEvent {
  userId: string;
  deviceId?: string | null;
  severity: TamperSeverity;
  risk: number;
  kind: string;
  title: string;
  details?: string | null;
  happenedAt: number;
}

const criticalLogTypes = new Set(['service_stop', 'daemon_stop_signal']);
const warningLogTypes = new Set(['system_shutdown', 'system_startup', 'session_logout']);
const infoLogTypes = new Set(['session_login']);

export function riskToSeverity(risk: number | null | undefined): TamperSeverity | null {
  if (risk == null) {
    return null;
  }

  if (risk >= 0.9) {
    return 'critical';
  }

  if (risk >= 0.6) {
    return 'warning';
  }

  if (risk > 0) {
    return 'info';
  }

  return null;
}

export function classifyDeviceLogEvent(input: {
  userId: string;
  deviceId: string;
  type: string;
  ts: number;
  data?: Record<string, unknown>;
}): ClassifiedRiskEvent | null {
  if (criticalLogTypes.has(input.type)) {
    return {
      userId: input.userId,
      deviceId: input.deviceId,
      severity: 'critical',
      risk: 1,
      kind: input.type,
      title: 'Monitoring stopped unexpectedly',
      details: `Device reported ${input.type.replaceAll('_', ' ')}.`,
      happenedAt: input.ts,
    };
  }

  if (warningLogTypes.has(input.type)) {
    return {
      userId: input.userId,
      deviceId: input.deviceId,
      severity: 'warning',
      risk: 0.7,
      kind: input.type,
      title: 'Monitoring interruption detected',
      details: `Device reported ${input.type.replaceAll('_', ' ')}.`,
      happenedAt: input.ts,
    };
  }

  if (infoLogTypes.has(input.type)) {
    return {
      userId: input.userId,
      deviceId: input.deviceId,
      severity: 'info',
      risk: 0.3,
      kind: input.type,
      title: 'Monitoring session changed',
      details: `Device reported ${input.type.replaceAll('_', ' ')}.`,
      happenedAt: input.ts,
    };
  }

  return null;
}

export function classifyUploadGap(input: {
  userId: string;
  deviceId: string;
  deviceName: string;
  gapMs: number;
  now: number;
  warningHours: number;
  criticalHours: number;
}): (ClassifiedRiskEvent & { dedupeWindowStart: number; dedupeWindowEnd: number }) | null {
  const warningMs = input.warningHours * 60 * 60 * 1000;
  const criticalMs = input.criticalHours * 60 * 60 * 1000;
  if (input.gapMs < warningMs) {
    return null;
  }

  const severity: TamperSeverity = input.gapMs >= criticalMs ? 'critical' : 'warning';
  const risk = severity === 'critical' ? 0.95 : 0.7;
  const bucketMs = severity === 'critical' ? criticalMs : warningMs;
  const bucket = Math.floor(input.now / Math.max(bucketMs, 1));
  const dedupeWindowStart = bucket * bucketMs;
  const dedupeWindowEnd = dedupeWindowStart + bucketMs;

  return {
    userId: input.userId,
    deviceId: input.deviceId,
    severity,
    risk,
    kind: 'upload_gap',
    title:
      severity === 'critical'
        ? `Long screenshot upload gap on ${input.deviceName}`
        : `Screenshot upload delay on ${input.deviceName}`,
    details: `No uploaded screenshot batch has been seen for ${Math.round(input.gapMs / (60 * 60 * 1000))} hour(s).`,
    happenedAt: input.now,
    dedupeWindowStart,
    dedupeWindowEnd,
  };
}

export async function notifyPartnersAboutRiskLog(
  db: D1Database,
  env: Env,
  input: {
    logId: string;
    appUrl?: string;
    userId: string;
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
    if ((target.send_digest ?? 1) !== 1) {
      continue;
    }

    const minimumSeverity =
      normalizeImmediateTamperSeverity(target.immediate_tamper_severity) ??
      DEFAULT_IMMEDIATE_TAMPER_SEVERITY;

    if (!severityAtLeast(input.severity, minimumSeverity)) {
      continue;
    }

    const email = renderTamperAlertTemplate({
      appName: env.APP_NAME,
      severity: input.severity,
      ownerName: owner.name,
      ownerEmail: owner.email,
      title: input.title,
      details: input.details ?? null,
      appUrl: input.appUrl ?? env.APP_URL,
    });

    await sendEmail({
      env,
      db,
      kind: 'tamper_alert',
      recipient: target.partner_email,
      subject: email.subject,
      text: email.text,
      html: email.html,
      related_user_id: input.userId,
      related_partnership_id: target.partnership_id,
      metadata: {
        cadence: target.digest_cadence ?? DEFAULT_DIGEST_CADENCE,
        logId: input.logId,
        risk: input.risk,
        severity: input.severity,
        happenedAt: input.happenedAt,
      },
    });
  }
}
