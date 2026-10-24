import { SESv2Client, SendEmailCommand } from '@aws-sdk/client-sesv2';

export interface SesConfig {
  region: string;
  accessKeyId: string;
  secretAccessKey: string;
}

export interface OutgoingEmail {
  // Full From header, e.g. `The Virtue Initiative <newsletter@news.example.org>`.
  from: string;
  to: string;
  subject: string;
  text: string;
  html: string;
  headers?: Record<string, string>;
  // SES configuration set whose event destination reports bounces and complaints.
  configurationSet?: string;
}

const clients = new Map<string, SESv2Client>();

function clientFor(config: SesConfig) {
  const key = `${config.region}:${config.accessKeyId}`;
  let client = clients.get(key);
  if (!client) {
    client = new SESv2Client({
      region: config.region,
      credentials: { accessKeyId: config.accessKeyId, secretAccessKey: config.secretAccessKey },
    });
    clients.set(key, client);
  }
  return client;
}

export async function sendSesEmail(config: SesConfig, email: OutgoingEmail): Promise<void> {
  await clientFor(config).send(
    new SendEmailCommand({
      FromEmailAddress: email.from,
      ConfigurationSetName: email.configurationSet,
      Destination: { ToAddresses: [email.to] },
      Content: {
        Simple: {
          Subject: { Data: email.subject },
          Body: { Text: { Data: email.text }, Html: { Data: email.html } },
          Headers: Object.entries(email.headers ?? {}).map(([Name, Value]) => ({ Name, Value })),
        },
      },
    }),
  );
}
