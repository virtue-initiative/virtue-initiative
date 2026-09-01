import { http, HttpResponse } from 'msw';
import { CURRENT_API_VERSION } from '@virtueinitiative/shared-web/api-version';
import { TEST_DEVICES, TEST_USER, TEST_WATCHER, TEST_WATCHING } from './fixtures';

const BASE = `http://localhost:8787/${CURRENT_API_VERSION}`;

const MOCK_HASH_PARAMS = {
  version: 'argon2id-v1',
  algorithm: 'argon2id',
  salt_length: 16,
  memory_cost_kib: 65536,
  time_cost: 3,
  parallelism: 1,
  hkdf_hash: 'sha256',
};

export const handlers = [
  // ── Login material (also serves current hash params when `email` is omitted) ──
  http.get(`${BASE}/user/login-material`, ({ request }) => {
    const url = new URL(request.url);
    if (!url.searchParams.has('email')) {
      return HttpResponse.json({ params: MOCK_HASH_PARAMS });
    }
    return HttpResponse.json({
      password_salt: btoa('testsalt12345678'),
      params: MOCK_HASH_PARAMS,
    });
  }),

  // ── Login ──────────────────────────────────────────────────────────────
  http.post(`${BASE}/login`, () => new HttpResponse(null, { status: 204 })),

  // ── Signup ─────────────────────────────────────────────────────────────
  http.post(`${BASE}/signup-request`, () => new HttpResponse(null, { status: 204 })),
  http.post(`${BASE}/signup`, () =>
    HttpResponse.json({
      user: { id: 'test-user-id', email: 'test@example.com', email_verified: true },
    }),
  ),
  http.post(`${BASE}/signup/validate`, () => HttpResponse.json({ email: 'test@example.com' })),

  // ── Logout ─────────────────────────────────────────────────────────────
  http.post(`${BASE}/logout`, () => new HttpResponse(null, { status: 204 })),

  // ── Password reset ─────────────────────────────────────────────────────
  http.post(`${BASE}/password-reset`, () => new HttpResponse(null, { status: 204 })),
  http.post(`${BASE}/password-reset/validate`, () =>
    HttpResponse.json({ email: 'test@example.com' }),
  ),
  http.post(`${BASE}/password-reset/finalize`, () => new HttpResponse(null, { status: 204 })),

  // ── User ───────────────────────────────────────────────────────────────
  http.get(`${BASE}/user`, () => HttpResponse.json(TEST_USER)),
  http.patch(`${BASE}/user`, () => HttpResponse.json({ email_verification_required: false })),
  http.delete(`${BASE}/user`, () => new HttpResponse(null, { status: 204 })),

  // ── Email verify ───────────────────────────────────────────────────────
  http.post(`${BASE}/user/verify-email`, () => new HttpResponse(null, { status: 204 })),

  // ── Devices ────────────────────────────────────────────────────────────
  http.get(`${BASE}/device`, () => HttpResponse.json(TEST_DEVICES)),
  http.patch(`${BASE}/device/:id`, () => new HttpResponse(null, { status: 204 })),
  http.delete(`${BASE}/device/:id`, () => new HttpResponse(null, { status: 204 })),

  // ── Partners ───────────────────────────────────────────────────────────
  http.get(`${BASE}/partner`, () =>
    HttpResponse.json({ watchers: [TEST_WATCHER], watching: [TEST_WATCHING] }),
  ),
  http.post(`${BASE}/partner`, () =>
    HttpResponse.json({ id: 'new-watching-1', invite_token: 'tok123' }),
  ),
  http.post(`${BASE}/partner/validate`, () =>
    HttpResponse.json({ owner: { id: 'some-user', name: 'Some User', email: 'some@example.com' } }),
  ),
  http.post(`${BASE}/partner/accept`, () => HttpResponse.json({ id: 'new-partner-1' })),
  http.delete(`${BASE}/partner/:id`, () => new HttpResponse(null, { status: 204 })),

  // ── Have I Been Pwned range search (third-party, called from the browser) ──
  http.get('https://api.pwnedpasswords.com/range/:prefix', () =>
    HttpResponse.text(
      '0000000000000000000000000000000000A:3\n0000000000000000000000000000000000B:0',
    ),
  ),

  // ── Data ───────────────────────────────────────────────────────────────
  http.get(`${BASE}/data`, () =>
    HttpResponse.json({ batches: [], user: TEST_USER, watching: [], watchers: [] }),
  ),
];
