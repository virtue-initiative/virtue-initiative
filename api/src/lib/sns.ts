import { importX509 } from 'jose';
import { z } from 'zod';

export const snsMessageSchema = z.object({
  Type: z.string(),
  MessageId: z.string(),
  TopicArn: z.string(),
  Message: z.string(),
  Timestamp: z.string(),
  SignatureVersion: z.enum(['1', '2']),
  Signature: z.string(),
  SigningCertURL: z.string(),
  Subject: z.string().nullish(),
  Token: z.string().optional(),
  SubscribeURL: z.string().optional(),
});

export type SnsMessage = z.infer<typeof snsMessageSchema>;

const SNS_HOST = /^sns\.[a-z0-9-]+\.amazonaws\.com$/;

// SPKI public keys by SigningCertURL. SNS rotates its cert rarely, so this stays tiny.
const signingKeyCache = new Map<string, ArrayBuffer>();
const SIGNING_KEY_CACHE_MAX = 8;

export function parseTopicArnAllowlist(value: string | undefined) {
  return (value ?? '')
    .split(',')
    .map((arn) => arn.trim())
    .filter(Boolean);
}

export function isSnsUrl(value: string) {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return false;
  }
  return (
    url.protocol === 'https:' &&
    url.port === '' &&
    url.username === '' &&
    url.password === '' &&
    SNS_HOST.test(url.hostname)
  );
}

// The fields SNS signs, in the order it signs them, as "Name\nValue\n" pairs.
function stringToSign(message: SnsMessage) {
  const fields: Array<[string, string | null | undefined]> =
    message.Type === 'Notification'
      ? [
          ['Message', message.Message],
          ['MessageId', message.MessageId],
          ['Subject', message.Subject],
          ['Timestamp', message.Timestamp],
          ['TopicArn', message.TopicArn],
          ['Type', message.Type],
        ]
      : [
          ['Message', message.Message],
          ['MessageId', message.MessageId],
          ['SubscribeURL', message.SubscribeURL],
          ['Timestamp', message.Timestamp],
          ['Token', message.Token],
          ['TopicArn', message.TopicArn],
          ['Type', message.Type],
        ];

  return fields
    .filter(([, value]) => value !== undefined && value !== null)
    .map(([name, value]) => `${name}\n${value}\n`)
    .join('');
}

async function loadSigningKey(certUrl: string) {
  const cached = signingKeyCache.get(certUrl);
  if (cached) {
    return cached;
  }

  const response = await fetch(certUrl, { redirect: 'manual' });
  if (!response.ok) {
    throw new Error(`SNS signing cert fetch failed with ${response.status}`);
  }

  const key = await importX509(await response.text(), 'RS256', { extractable: true });
  const spki = (await crypto.subtle.exportKey('spki', key as CryptoKey)) as ArrayBuffer;

  if (signingKeyCache.size >= SIGNING_KEY_CACHE_MAX) {
    signingKeyCache.clear();
  }
  signingKeyCache.set(certUrl, spki);
  return spki;
}

/** API-041: true only if `message` was signed by the cert at its (AWS-hosted) `SigningCertURL`. */
export async function verifySnsSignature(message: SnsMessage) {
  if (!isSnsUrl(message.SigningCertURL)) {
    return false;
  }

  try {
    const key = await crypto.subtle.importKey(
      'spki',
      await loadSigningKey(message.SigningCertURL),
      { name: 'RSASSA-PKCS1-v1_5', hash: message.SignatureVersion === '1' ? 'SHA-1' : 'SHA-256' },
      false,
      ['verify'],
    );
    const signature = Uint8Array.from(atob(message.Signature), (char) => char.charCodeAt(0));

    return await crypto.subtle.verify(
      'RSASSA-PKCS1-v1_5',
      key,
      signature,
      new TextEncoder().encode(stringToSign(message)),
    );
  } catch (error) {
    console.error('SNS signature verification failed', error);
    return false;
  }
}
