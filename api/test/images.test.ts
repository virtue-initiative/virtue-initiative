// Image route integration tests. R2 is mocked (presigned URL generation).
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { SELF } from 'cloudflare:test';
import { BASE, clearDB, signupAndGetToken, authHeaders } from './helpers';

vi.mock('../src/lib/r2', () => ({
  generateUploadUrl: vi.fn().mockResolvedValue('https://r2.example.com/upload?sig=fake'),
  generateDownloadUrl: vi.fn().mockResolvedValue('https://r2.example.com/download?sig=fake'),
  putObject: vi.fn().mockResolvedValue(undefined),
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

describe('POST /image', () => {
  it('creates image metadata and returns a presigned upload URL', async () => {
    const { token } = await signupAndGetToken('alice@example.com');
    const deviceId = await createDevice(token);

    const res = await SELF.fetch(`${BASE}/image`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({
        device_id: deviceId,
        sha256: VALID_SHA256,
        content_type: 'image/jpeg',
        size_bytes: 204800,
        taken_at: new Date().toISOString(),
      }),
    });
    expect(res.status).toBe(201);
    const body = (await res.json()) as {
      image: { id: string; status: string };
      upload_url: string;
    };
    expect(body.image.id).toBeTruthy();
    expect(body.image.status).toBe('pending_upload');
    expect(body.upload_url).toContain('https://');
  });

  it('returns 404 when device_id does not belong to the user', async () => {
    const { token } = await signupAndGetToken('bob@example.com');
    const res = await SELF.fetch(`${BASE}/image`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({
        device_id: 'non-existent-device',
        sha256: VALID_SHA256,
        content_type: 'image/png',
        size_bytes: 1024,
        taken_at: new Date().toISOString(),
      }),
    });
    expect(res.status).toBe(404);
  });

  it('returns 400 for missing required fields', async () => {
    const { token } = await signupAndGetToken('carol@example.com');
    const res = await SELF.fetch(`${BASE}/image`, {
      method: 'POST',
      headers: authHeaders(token),
      body: JSON.stringify({ device_id: 'some-id' }), // missing sha256, content_type etc.
    });
    expect(res.status).toBe(400);
  });

  it('returns 401 without auth', async () => {
    const res = await SELF.fetch(`${BASE}/image`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    });
    expect(res.status).toBe(401);
  });
});

