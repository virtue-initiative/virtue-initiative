import { fetchMock } from 'cloudflare:test';

/**
 * In-memory test double for the standalone hash server (see ../../hash-server/SPEC.md),
 * installed via fetchMock so lib/hash-server.ts's real fetch() calls (GET and DELETE —
 * the only ones the API itself makes; devices POST directly to the real hash server,
 * never through this API, so there's nothing here to intercept for that) hit this
 * instead of the network.
 *
 * Installed once for the whole file by test/setup.ts. State lives in this module-level
 * map, not per-call closure state, so individual test files can seed/inspect it with
 * seedHashState()/getHashState() — and must reset it via resetHashServerMock() in their
 * own clearDB()/beforeEach, since it otherwise persists across tests within the same
 * file like any other module global.
 */
export const HASH_SERVER_ORIGIN = 'https://example.com';

export interface MockHashState {
  hash: string;
  seq: number;
  last_received: number;
}

const ZERO_STATE: MockHashState = { hash: '0'.repeat(64), seq: 0, last_received: 0 };

const states = new Map<string, MockHashState>();

export function resetHashServerMock(): void {
  states.clear();
}

export function seedHashState(deviceId: string, state: MockHashState): void {
  states.set(deviceId, state);
}

export function getHashState(deviceId: string): MockHashState {
  return states.get(deviceId) ?? ZERO_STATE;
}

function decodeJwtPayload(token: string): { sub?: string; type?: string } {
  const payload = token.split('.')[1] ?? '';
  const padded = payload.padEnd(payload.length + ((4 - (payload.length % 4)) % 4), '=');
  const base64 = padded.replace(/-/g, '+').replace(/_/g, '/');
  try {
    return JSON.parse(atob(base64)) as { sub?: string; type?: string };
  } catch {
    return {};
  }
}

function bearerClaims(headers: unknown): { sub?: string; type?: string } {
  const record = headers as Record<string, string> | undefined;
  const raw = record?.authorization ?? record?.Authorization ?? '';
  const token = raw.replace(/^Bearer\s+/i, '');
  return token ? decodeJwtPayload(token) : {};
}

export function installHashServerMock(): void {
  fetchMock
    .get(HASH_SERVER_ORIGIN)
    .intercept({ path: () => true, method: () => true })
    .reply((opts) => {
      const url = new URL(opts.path, HASH_SERVER_ORIGIN);
      const claims = bearerClaims(opts.headers);

      if (!claims.type) {
        return {
          statusCode: 401,
          data: JSON.stringify({ code: 'unauthorized', message: 'no token' }),
        };
      }

      if (opts.method === 'GET') {
        const ids = (url.searchParams.get('devices') ?? '').split(',').filter(Boolean);
        const body: Record<string, MockHashState> = {};
        for (const id of ids) body[id] = states.get(id) ?? ZERO_STATE;
        return { statusCode: 200, data: JSON.stringify(body) };
      }

      if (opts.method === 'DELETE') {
        const id = url.searchParams.get('device') ?? '';
        const prior = states.get(id) ?? ZERO_STATE;
        states.set(id, { ...ZERO_STATE });
        return { statusCode: 200, data: JSON.stringify(prior) };
      }

      return { statusCode: 404, data: '' };
    })
    .persist();
}
