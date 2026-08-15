import { CURRENT_API_VERSION } from './api-version';
import { generateToken } from './jwt';
import { Env } from '../types/bindings';

export interface HashState {
  hash: string; // hex-encoded 32-byte hash
  seq: number;
  last_received: number;
}

const ZERO_HASH_STATE: HashState = { hash: '0'.repeat(64), seq: 0, last_received: 0 };

// The whole codebase shares one version (see api-version.ts), which is also what the
// hash server expects its own requests prefixed with (hash-server/SPEC.md section 1.3).
function baseUrl(env: Env): string {
  const url = env.HASH_SERVER_URL?.trim();
  if (!url) {
    throw new Error('HASH_SERVER_URL is not configured');
  }
  return `${url.replace(/\/+$/, '')}/${CURRENT_API_VERSION}`;
}

/** Fetches hash-chain state for a single device. Zero-filled if unknown to the hash server. */
export async function hashGet(env: Env, deviceId: string): Promise<HashState> {
  const many = await hashGetMany(env, [deviceId]);
  return many.get(deviceId) ?? ZERO_HASH_STATE;
}

/**
 * Fetches hash-chain state for many devices in a single request — the batched
 * `GET /hash?devices=...` the hash server exists to support (see hash-server/SPEC.md
 * section 2.2), used by the device list view instead of one request per device.
 */
export async function hashGetMany(env: Env, deviceIds: string[]): Promise<Map<string, HashState>> {
  const result = new Map<string, HashState>();
  if (deviceIds.length === 0) {
    return result;
  }

  const token = await generateToken('server', 'api', env.JWT_PRIVATE_KEY, 60);
  const resp = await fetch(`${baseUrl(env)}/hash?devices=${deviceIds.join(',')}`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!resp.ok) {
    throw new Error(`hash server GET /hash failed: ${resp.status}`);
  }

  const body = (await resp.json()) as Record<string, HashState>;
  for (const [deviceId, state] of Object.entries(body)) {
    result.set(deviceId, state);
  }
  return result;
}

/** Resets a device's hash-chain state to zero, returning the state from before the reset. */
export async function hashReset(env: Env, deviceId: string): Promise<HashState> {
  const token = await generateToken('server', 'api', env.JWT_PRIVATE_KEY, 60);
  const resp = await fetch(`${baseUrl(env)}/hash?device=${deviceId}`, {
    method: 'DELETE',
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!resp.ok) {
    throw new Error(`hash server DELETE /hash failed: ${resp.status}`);
  }
  return (await resp.json()) as HashState;
}
