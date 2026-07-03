import { http, HttpResponse } from 'msw';
import { TEST_DEVICES, TEST_USER, TEST_WATCHER, TEST_WATCHING } from './fixtures';

const BASE = 'http://localhost:8787';

export const handlers = [
  // ── Hash params ────────────────────────────────────────────────────────
  http.get(`${BASE}/current-hash-params`, () =>
    HttpResponse.json({
      salt_length: 16,
      memory_cost_kib: 65536,
      time_cost: 3,
      parallelism: 1,
    }),
  ),

  // ── Login material ─────────────────────────────────────────────────────
  http.get(`${BASE}/user/login-material`, () =>
    HttpResponse.json({
      password_salt: btoa('testsalt12345678'),
      memory_cost_kib: 65536,
      time_cost: 3,
      parallelism: 1,
    }),
  ),

  // ── Login ──────────────────────────────────────────────────────────────
  http.post(`${BASE}/login`, () => HttpResponse.json({ ok: true, refresh_token: 'mock-token' })),

  // ── Signup ─────────────────────────────────────────────────────────────
  http.post(`${BASE}/signup-request`, () => HttpResponse.json({ ok: true })),
  http.post(`${BASE}/signup`, () =>
    HttpResponse.json({
      user: { id: 'test-user-id', email: 'test@example.com', email_verified: true },
    }),
  ),

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
  http.patch(`${BASE}/device/:id`, async ({ request, params }) => {
    const body = (await request.json()) as { name?: string };
    const device = TEST_DEVICES.find((d) => d.id === params.id);
    return HttpResponse.json({ ...device, ...(body.name ? { name: body.name } : {}) });
  }),
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
  http.delete(`${BASE}/partner/watcher/:id`, () => new HttpResponse(null, { status: 204 })),
  http.delete(`${BASE}/partner/watching/:id`, () => new HttpResponse(null, { status: 204 })),

  // ── Data ───────────────────────────────────────────────────────────────
  http.get(`${BASE}/data`, () => HttpResponse.json({ batches: [] })),
];
