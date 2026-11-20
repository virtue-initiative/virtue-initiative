import { findUserById, listAcceptedNotificationTargetsForUser } from './db';
import { sendEmail } from './email';
import { DnsFilterCheckChange, renderDnsFilterCheckChangedTemplate } from './email/templates';
import { Env } from '../types/bindings';

/**
 * API-020, API-030: emails every accepted watcher at once that the owner weakened their
 * DNS filter check, with the same delivery as a tamper alert. Best-effort: a failure is
 * logged and never fails the request that triggered it.
 */
export async function notifyWatchersAboutDnsFilterCheck(
  db: D1Database,
  env: Env,
  ownerId: string,
  change: DnsFilterCheckChange,
) {
  try {
    const owner = await findUserById(db, ownerId);
    if (!owner) {
      return;
    }

    const targets = await listAcceptedNotificationTargetsForUser(db, ownerId);
    for (const target of targets) {
      if (target.settings.email_frequency === 'none') {
        continue;
      }

      const email = renderDnsFilterCheckChangedTemplate({
        appName: env.APP_NAME,
        appUrl: env.APP_URL,
        ownerName: owner.name,
        ownerEmail: owner.email,
        change,
      });

      await sendEmail({
        env,
        db,
        kind: 'dns_filter_check_changed',
        recipient: target.watcher_email,
        recipientEmailVerified: target.watcher_email_verified,
        subject: email.subject,
        text: email.text,
        html: email.html,
        related_user_id: ownerId,
        related_partnership_id: target.partnership_id,
        metadata: { ...change },
      });
    }
  } catch (err) {
    console.error('DNS filter check alert failed', err);
  }
}
