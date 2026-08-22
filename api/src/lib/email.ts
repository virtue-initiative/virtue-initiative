import { SESv2Client, SendEmailCommand } from '@aws-sdk/client-sesv2';
import { v4 as uuidv4 } from 'uuid';
import { EmailKind } from './email-domain';
import { findUserByEmail } from './db';
import { Env } from '../types/bindings';

export interface EmailContent {
  subject: string;
  text: string;
  html: string;
}

export interface EmailAttachment {
  fileName: string;
  contentType: string;
  data: Uint8Array;
}

export interface MockEmailDelivery {
  kind: EmailKind;
  recipient_email: string;
  subject: string;
  text: string;
  html: string;
  status: 'sent' | 'failed' | 'skipped';
  metadata: string;
  attachmentFileNames: string[];
}

interface SendEmailInput extends EmailContent {
  env: Env;
  db: D1Database;
  kind: EmailKind;
  recipient: string;
  related_user_id?: string;
  related_partnership_id?: string;
  metadata?: Record<string, unknown>;
  allowUnverified?: boolean;
  replyTo?: string;
  attachments?: EmailAttachment[];
  // Pass this when the caller already looked up the recipient (e.g. a fan-out
  // over rows from a single batch query) to avoid a redundant per-call
  // findUserByEmail lookup here.
  recipientEmailVerified?: number;
}

let sesClient: SESv2Client | null = null;
const mockEmailOutbox: MockEmailDelivery[] = [];
const FROM_DISPLAY_NAME = 'The Virtue Initiative';

function withDisplayName(fromEmail: string) {
  return fromEmail.includes('<') ? fromEmail : `${FROM_DISPLAY_NAME} <${fromEmail}>`;
}

function getSesClient(env: Env) {
  if (!sesClient) {
    sesClient = new SESv2Client({
      region: env.AWS_SES_REGION,
      credentials: {
        accessKeyId: env.AWS_ACCESS_KEY_ID,
        secretAccessKey: env.AWS_SECRET_ACCESS_KEY,
      },
    });
  }

  return sesClient;
}

export async function sendEmail(input: SendEmailInput) {
  const id = uuidv4();
  const recipientEmailVerified =
    input.recipientEmailVerified ??
    (await findUserByEmail(input.db, input.recipient))?.email_verified;
  if (
    !input.allowUnverified &&
    input.kind !== 'email_verification' &&
    input.kind !== 'partner_invite' &&
    recipientEmailVerified !== undefined &&
    recipientEmailVerified !== 1
  ) {
    console.info('email delivery skipped for unverified recipient', {
      kind: input.kind,
      recipient: input.recipient,
      subject: input.subject,
    });
    return { id: `skipped-${id}` };
  }

  const attachmentFileNames = (input.attachments ?? []).map((a) => a.fileName);

  if (input.env.EMAIL_DELIVERY_MODE === 'log') {
    console.info('email delivery logged', {
      kind: input.kind,
      recipient: input.recipient,
      replyTo: input.replyTo,
      subject: input.subject,
      text: input.text,
      html: input.html,
      metadata: input.metadata ?? {},
      attachmentFileNames,
    });
    mockEmailOutbox.push({
      kind: input.kind,
      recipient_email: input.recipient,
      subject: input.subject,
      text: input.text,
      html: input.html,
      status: 'sent',
      metadata: JSON.stringify(input.metadata ?? {}),
      attachmentFileNames,
    });
    return { id: `mock-${id}` };
  }

  try {
    const response = await getSesClient(input.env).send(
      new SendEmailCommand({
        FromEmailAddress: withDisplayName(input.env.AWS_SES_FROM_EMAIL),
        Destination: { ToAddresses: [input.recipient] },
        ReplyToAddresses: input.replyTo ? [input.replyTo] : undefined,
        Content: {
          Simple: {
            Subject: { Data: input.subject },
            Body: {
              Text: { Data: input.text },
              Html: { Data: input.html },
            },
            Attachments: input.attachments?.map((a) => ({
              FileName: a.fileName,
              ContentType: a.contentType,
              RawContent: a.data,
            })),
          },
        },
      }),
    );

    return { id: response.MessageId ?? id };
  } catch (error) {
    mockEmailOutbox.push({
      kind: input.kind,
      recipient_email: input.recipient,
      subject: input.subject,
      text: input.text,
      html: input.html,
      status: 'failed',
      metadata: JSON.stringify(input.metadata ?? {}),
      attachmentFileNames,
    });
    throw error;
  }
}

export function listMockEmailDeliveries() {
  return [...mockEmailOutbox];
}

export function clearMockEmailDeliveries() {
  mockEmailOutbox.length = 0;
}
