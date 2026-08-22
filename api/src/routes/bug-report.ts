import { Context, Hono } from 'hono';
import { getCookie } from 'hono/cookie';
import { z } from 'zod';
import { rateLimitByIp } from '../middleware/rate-limit';
import { validateZ } from '../middleware/validation';
import { findDeviceById, findSessionByRefreshTokenHash, findUserById } from '../lib/db';
import { renderBugReportTemplate } from '../lib/email/templates';
import { sendEmail } from '../lib/email';
import { jsonField } from '../lib/form-validation';
import { assertTokenPurpose, hashOpaqueToken } from '../lib/tokens';
import { bugReportSchema } from '../../../shared-web/types';
import { Env, Variables } from '../types/bindings';

const bugReport = new Hono<{ Bindings: Env; Variables: Variables }>();

// SES's Simple-content attachment limit tracks its ~10MB total-message quota;
// stay comfortably under that once the text/HTML body and MIME overhead are added.
const MAX_LOG_ATTACHMENT_BYTES = 8 * 1024 * 1024;

const bugReportFormSchema = z.object({
  metadata: jsonField(bugReportSchema, 'metadata'),
  log_file: z
    .instanceof(File)
    .refine((file) => file.size <= MAX_LOG_ATTACHMENT_BYTES, {
      message: `log_file exceeds the ${MAX_LOG_ATTACHMENT_BYTES} byte limit`,
    })
    .optional(),
});

type ReporterIdentity =
  | { kind: 'user'; id: string; email?: string }
  | { kind: 'device'; id: string; name?: string; platform?: string; ownerEmail?: string };

/** Best-effort auth: reports are accepted with or without a token, so failures here just mean "anonymous". */
async function resolveReporterIdentity(
  c: Context<{ Bindings: Env; Variables: Variables }>,
): Promise<ReporterIdentity | null> {
  const cookieToken = getCookie(c, 'refresh_token');
  const authHeader = c.req.header('Authorization');
  const bearerToken = authHeader?.startsWith('Bearer ') ? authHeader.slice(7) : undefined;
  const token = cookieToken ?? bearerToken;
  if (!token) return null;

  try {
    assertTokenPurpose(token, 'web_session');
    const session = await findSessionByRefreshTokenHash(c.env.DB, hashOpaqueToken(token), 'web');
    if (session?.user_id && session.expires_at > Date.now()) {
      const user = await findUserById(c.env.DB, session.user_id);
      return { kind: 'user', id: session.user_id, email: user?.email };
    }
    return null;
  } catch {
    // Not a web session token; fall through and try a device token below.
  }

  try {
    assertTokenPurpose(token, 'device_session');
    const session = await findSessionByRefreshTokenHash(c.env.DB, hashOpaqueToken(token), 'device');
    if (session?.device_id && session.expires_at > Date.now()) {
      const device = await findDeviceById(c.env.DB, session.device_id);
      const owner = device?.owner ? await findUserById(c.env.DB, device.owner) : undefined;
      return {
        kind: 'device',
        id: session.device_id,
        name: device?.name,
        platform: device?.platform,
        ownerEmail: owner?.email,
      };
    }
  } catch {
    // Not a recognizable token either; treat the report as anonymous.
  }

  return null;
}

bugReport.post(
  '/bug-report',
  rateLimitByIp(),
  validateZ('form', bugReportFormSchema),
  async (c) => {
    const { metadata: body, log_file: logFile } = c.req.valid('form');
    const identity = await resolveReporterIdentity(c);

    const reporter =
      identity?.kind === 'user'
        ? `User ${identity.email ?? identity.id}`
        : identity?.kind === 'device'
          ? `Device "${identity.name ?? identity.id}" (${identity.platform ?? 'unknown platform'})` +
            (identity.ownerEmail ? ` owned by ${identity.ownerEmail}` : '')
          : 'Anonymous (no session)';

    const identityEmail =
      identity?.kind === 'user'
        ? identity.email
        : identity?.kind === 'device'
          ? identity.ownerEmail
          : undefined;
    const contactEmail = body.contact_email ?? identityEmail;

    // The client MAY supply richer platform_details (e.g. the Linux client sends
    // kernel/os-release info); when it doesn't, fall back to the User-Agent the
    // request itself carried, which is the best a browser can offer.
    const platformDetails = body.platform_details ?? c.req.header('User-Agent') ?? undefined;

    const template = renderBugReportTemplate({
      message: body.message,
      contactEmail,
      reporter,
      platform: body.platform ?? (identity?.kind === 'device' ? identity.platform : undefined),
      appVersion: body.app_version,
      platformDetails,
      hasLogAttachment: Boolean(logFile),
    });

    const attachments = logFile
      ? [
          {
            fileName: 'recent-logs.txt',
            contentType: 'text/plain',
            data: new Uint8Array(await logFile.arrayBuffer()),
          },
        ]
      : undefined;

    await sendEmail({
      env: c.env,
      db: c.env.DB,
      kind: 'bug_report',
      recipient: c.env.BUG_REPORT_EMAIL,
      subject: template.subject,
      text: template.text,
      html: template.html,
      allowUnverified: true,
      replyTo: contactEmail,
      attachments,
      metadata: { platform: body.platform, app_version: body.app_version, platformDetails },
    });

    return c.body(null, 204);
  },
);

export default bugReport;
