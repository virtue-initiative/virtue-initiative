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

export interface MockEmailDelivery {
  kind: EmailKind;
  recipient_email: string;
  subject: string;
  text: string;
  html: string;
  status: 'sent' | 'failed' | 'skipped';
  metadata: string;
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
}

let sesClient: SESv2Client | null = null;
const mockEmailOutbox: MockEmailDelivery[] = [];

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
  const recipientUser = await findUserByEmail(input.db, input.recipient);
  if (
    !input.allowUnverified &&
    input.kind !== 'email_verification' &&
    input.kind !== 'partner_invite' &&
    recipientUser &&
    recipientUser.email_verified !== 1
  ) {
    console.info('email delivery skipped for unverified recipient', {
      kind: input.kind,
      recipient: input.recipient,
      subject: input.subject,
    });
    return { id: `skipped-${id}` };
  }

  if (input.env.EMAIL_DELIVERY_MODE !== 'ses') {
    console.info('email delivery skipped', {
      kind: input.kind,
      recipient: input.recipient,
      subject: input.subject,
    });
    mockEmailOutbox.push({
      kind: input.kind,
      recipient_email: input.recipient,
      subject: input.subject,
      text: input.text,
      html: input.html,
      status: 'sent',
      metadata: JSON.stringify(input.metadata ?? {}),
    });
    return { id: `mock-${id}` };
  }

  try {
    const response = await getSesClient(input.env).send(
      new SendEmailCommand({
        FromEmailAddress: input.env.AWS_SES_FROM_EMAIL,
        Destination: { ToAddresses: [input.recipient] },
        Content: {
          Simple: {
            Subject: { Data: input.subject },
            Body: {
              Text: { Data: input.text },
              Html: { Data: input.html },
            },
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
