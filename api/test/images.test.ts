// Image route integration tests. R2 is mocked (native binding).
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { SELF } from 'cloudflare:test';
import { BASE, clearDB, signupAndGetToken, authHeaders } from './helpers';

vi.mock('../src/lib/r2', () => ({
  putObject: vi.fn().mockResolvedValue(undefined),
  getObject: vi.fn().mockResolvedValue({
    body: new ReadableStream(),
    httpMetadata: { contentType: 'image/webp' },
  }),
  objectExists: vi.fn().mockResolvedValue(false),
}));

beforeEach(clearDB);

const VALID_SHA256 = 'a'.repeat(64); // valid 64-char hex string for tests
async function createDevice(token: string): Promise<string> {
  const res = await SELF.fetch(`${BASE}/device`, {
    method: 'POST',
    headers: authHeaders(token),
    body: JSON.stringify({ name: 'Test Device', platform: 'linux', avg_interval_seconds: 300 }),
  });
  return ((await res.json()) as { id: string }).id;
}

function makeImageForm(deviceId: string, overrides: Record<string, string> = {}): FormData {
  const form = new FormData();
  form.set('device_id', deviceId);
  form.set('sha256', VALID_SHA256);
  form.set('taken_at', new Date().toISOString());
  form.set('file', new File([new Uint8Array(100)], 'screen.webp', { type: 'image/webp' }));
  for (const [k, v] of Object.entries(overrides)) form.set(k, v);
  return form;
}

describe('POST /image', () => {
  it('uploads image and returns metadata', async () => {
    const { token } = await signupAndGetToken('alice@example.com');
    const deviceId = await createDevice(token);

    const res = await SELF.fetch(`${BASE}/image`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}` },
      body: makeImageForm(deviceId),
    });
    expect(res.status).toBe(201);
    const body = (await res.json()) as { image: { id: string; status: string } };
    expect(body.image.id).toBeTruthy();
    expect(body.image.status).toBe('uploaded');
  });

  it('returns 404 when device_id does not belong to the user', async () => {
    const { token } = await signupAndGetToken('bob@example.com');
    const res = await SELF.fetch(`${BASE}/image`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}` },
      body: makeImageForm('non-existent-device'),
    });
    expect(res.status).toBe(404);
  });

  it('returns 400 when file field is missing', async () => {
    const { token } = await signupAndGetToken('carol@example.com');
    const deviceId = await createDevice(token);
    const form = new FormData();
    form.set('device_id', deviceId);
    form.set('sha256', VALID_SHA256);
    form.set('taken_at', new Date().toISOString());
    const res = await SELF.fetch(`${BASE}/image`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}` },
      body: form,
    });
    expect(res.status).toBe(400);
  });

  it('returns 401 without auth', async () => {
    const res = await SELF.fetch(`${BASE}/image`, {
      method: 'POST',
      body: new FormData(),
    });
    expect(res.status).toBe(401);
  });
});

describe('GET /image/:id', () => {
  it('returns 404 for non-existent image', async () => {
    const { token } = await signupAndGetToken('dave@example.com');
    const res = await SELF.fetch(`${BASE}/image/non-existent-id`, {
      headers: authHeaders(token),
    });
    expect(res.status).toBe(404);
  });

  it('returns 401 without auth', async () => {
    const res = await SELF.fetch(`${BASE}/image/some-id`);
    expect(res.status).toBe(401);
  });
});

