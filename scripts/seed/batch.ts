// Builds an encrypted log batch byte-for-byte the way client/core does, so the
// web app decrypts and verifies it like a real upload:
//
//   each event:  msgpack({ts, risk?, type, data?})
//   payload:     msgpack([eventBytes, ...]) → gzip → AES-256-GCM(batchKey)
//   wire:        nonce[12] || ciphertext+tag
//   access_keys: {recipientUuid: base64(HPKE seal of batchKey to their pub_key)}
//   end_hash:    hash chain from 32 zero bytes, state = sha256(state || sha256(eventBytes))
//
// The end_hash is what the hash server would hold at upload time. The web
// verifies each batch from a zero start state (web/src/utils/cache/worker.ts),
// so a correct chain here makes seeded batches show as verified.
import { encode } from '@msgpack/msgpack';
import { gzipSync } from 'zlib';
import { createHash } from 'crypto';
import {
  encryptData,
  encryptForPublicKey,
  generateRandomKeyBytes,
} from '../../web/src/utils/api/crypto';
import { toHex } from './util';

export interface SeedEvent {
  ts: number;
  risk?: number;
  type: string;
  data?: Record<string, unknown>;
}

export interface EncryptedBatch {
  blob: Uint8Array;
  endHashHex: string;
  accessKeys: Record<string, string>;
  highRiskCount: number;
  mediumRiskCount: number;
}

function sha256(data: Uint8Array): Buffer {
  return createHash('sha256').update(data).digest();
}

export async function buildBatch(
  events: SeedEvent[],
  recipients: { uuid: string; pubKey: Uint8Array }[],
): Promise<EncryptedBatch> {
  const eventBytes = events.map((event) => {
    // Key order mirrors the Rust LogEntry: ts, risk, then the flattened
    // {type, data} tag. risk and data are omitted when absent, like serde does.
    const entry: Record<string, unknown> = { ts: event.ts };
    if (event.risk !== undefined) entry.risk = event.risk;
    entry.type = event.type;
    if (event.data !== undefined) entry.data = event.data;
    return encode(entry);
  });

  let state: Buffer = Buffer.alloc(32);
  for (const bytes of eventBytes) {
    state = sha256(Buffer.concat([state, sha256(bytes)]));
  }

  const batchKeyBytes = generateRandomKeyBytes();
  const batchKey = await crypto.subtle.importKey('raw', batchKeyBytes, 'AES-GCM', false, [
    'encrypt',
  ]);
  const blob = await encryptData(batchKey, gzipSync(encode(eventBytes)));

  const accessKeys: Record<string, string> = {};
  for (const { uuid, pubKey } of recipients) {
    const sealed = await encryptForPublicKey(pubKey.slice(), batchKeyBytes);
    accessKeys[uuid] = Buffer.from(sealed).toString('base64');
  }

  // Same thresholds the client uses for BatchUpload event_counts (≥0.7 high,
  // 0.4–0.7 medium), which feed the partner digest emails.
  const risks = events.map((e) => e.risk ?? 0);
  return {
    blob,
    endHashHex: toHex(state),
    accessKeys,
    highRiskCount: risks.filter((r) => r >= 0.7).length,
    mediumRiskCount: risks.filter((r) => r >= 0.4 && r < 0.7).length,
  };
}
