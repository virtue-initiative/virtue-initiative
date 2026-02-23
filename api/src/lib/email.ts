import { Env } from '../types/bindings';

/**
 * Send a generic email. Stub — replace with your email provider (e.g. Resend, MailChannels).
 */
export async function sendEmail(
  _env: Env,
  to: string,
  subject: string,
  text: string,
): Promise<void> {
  console.log(`[email stub] To: ${to} | Subject: ${subject}\n${text}`);
}

/**
 * Notify both parties when a partnership is deleted.
 */
export async function sendPartnerDeletionEmail(
  env: Env,
  deletedByEmail: string,
  otherEmail: string,
): Promise<void> {
  await sendEmail(
    env,
    otherEmail,
    'Your accountability partnership has ended',
    `Hi,\n\n${deletedByEmail} has removed you as an accountability partner on BePure.\n\nIf you have questions, you can reach out to them directly.\n\n— The BePure team`,
  );
  await sendEmail(
    env,
    deletedByEmail,
    'Accountability partnership removed',
    `Hi,\n\nYou have successfully removed your accountability partnership with ${otherEmail} on BePure.\n\n— The BePure team`,
  );
}
