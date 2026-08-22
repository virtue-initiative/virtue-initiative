import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import {
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  listEmailDeliveries,
  signupAndGetCookie,
} from './helpers';

beforeEach(clearDB);

function bugReportForm(
  metadata: Record<string, unknown>,
  logFile?: File,
): { form: FormData; headers?: Record<string, string> } {
  const form = new FormData();
  form.set('metadata', JSON.stringify(metadata));
  if (logFile) form.set('log_file', logFile);
  return { form };
}

describe('POST /bug-report', () => {
  it('accepts an anonymous report and emails the fixed support address', async () => {
    const { form } = bugReportForm({
      message: 'The app crashes on launch.',
      contact_email: 'reporter@example.com',
      platform: 'linux',
      app_version: '1.2.3',
    });
    const res = await SELF.fetch(`${BASE}/bug-report`, { method: 'POST', body: form });

    expect(res.status).toBe(204);

    const deliveries = await listEmailDeliveries();
    const report = deliveries.find((d) => d.kind === 'bug_report');
    expect(report).toBeTruthy();
    expect(report!.recipient_email).toBe('help@virtueinitiative.org');
    expect(report!.text).toContain('The app crashes on launch.');
    expect(report!.text).toContain('reporter@example.com');
    expect(report!.text).toContain('linux');
    expect(report!.text).toContain('1.2.3');
    expect(report!.text).toContain('Anonymous');
    expect(report!.attachmentFileNames).toEqual([]);
  });

  it('attaches the authenticated user to the report when a web session is present', async () => {
    const { cookie } = await signupAndGetCookie('bug-reporter@example.com');
    const { form } = bugReportForm({ message: 'Something is broken.' });

    const res = await SELF.fetch(`${BASE}/bug-report`, {
      method: 'POST',
      headers: { Cookie: authHeaders(cookie).Cookie },
      body: form,
    });

    expect(res.status).toBe(204);

    const deliveries = await listEmailDeliveries();
    const report = deliveries.find((d) => d.kind === 'bug_report');
    expect(report).toBeTruthy();
    expect(report!.text).toContain('bug-reporter@example.com');
  });

  it('attaches the authenticated device to the report when a device token is present', async () => {
    await signupAndGetCookie('device-owner@example.com');
    const device = await createDeviceForUser(
      'device-owner@example.com',
      'password123',
      'Bug Report Laptop',
      'linux',
    );
    const { form } = bugReportForm({ message: 'Screenshot capture failed.' });

    const res = await SELF.fetch(`${BASE}/bug-report`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${device.refresh_token}` },
      body: form,
    });

    expect(res.status).toBe(204);

    const deliveries = await listEmailDeliveries();
    const report = deliveries.find((d) => d.kind === 'bug_report');
    expect(report).toBeTruthy();
    expect(report!.text).toContain('Bug Report Laptop');
    expect(report!.text).toContain('linux');
    expect(report!.text).toContain('device-owner@example.com');
  });

  it('uses the sent platform_details when the client provides one', async () => {
    const { form } = bugReportForm({
      message: 'Kernel panic on screenshot capture.',
      platform_details: 'Linux 6.8.0-60-lowlatency; Ubuntu 24.04',
    });

    const res = await SELF.fetch(`${BASE}/bug-report`, {
      method: 'POST',
      headers: { 'User-Agent': 'ShouldBeIgnored/1.0' },
      body: form,
    });

    expect(res.status).toBe(204);

    const deliveries = await listEmailDeliveries();
    const report = deliveries.find((d) => d.kind === 'bug_report');
    expect(report!.text).toContain('Linux 6.8.0-60-lowlatency; Ubuntu 24.04');
    expect(report!.text).not.toContain('ShouldBeIgnored/1.0');
  });

  it('falls back to the User-Agent header when platform_details is omitted', async () => {
    const { form } = bugReportForm({ message: 'Button does nothing when clicked.' });

    const res = await SELF.fetch(`${BASE}/bug-report`, {
      method: 'POST',
      headers: { 'User-Agent': 'Mozilla/5.0 (Test Browser)' },
      body: form,
    });

    expect(res.status).toBe(204);

    const deliveries = await listEmailDeliveries();
    const report = deliveries.find((d) => d.kind === 'bug_report');
    expect(report!.text).toContain('Mozilla/5.0 (Test Browser)');
  });

  it('attaches a sent log_file to the email and notes it in the body', async () => {
    const logFile = new File(['line one\nline two\n'], 'recent-logs.txt', {
      type: 'text/plain',
    });
    const { form } = bugReportForm({ message: 'Upload seems stuck.' }, logFile);

    const res = await SELF.fetch(`${BASE}/bug-report`, { method: 'POST', body: form });

    expect(res.status).toBe(204);

    const deliveries = await listEmailDeliveries();
    const report = deliveries.find((d) => d.kind === 'bug_report');
    expect(report!.attachmentFileNames).toEqual(['recent-logs.txt']);
    expect(report!.text).toContain('Attached: recent-logs.txt');
  });

  it('rejects a log_file above the size limit', async () => {
    const oversized = new File([new Uint8Array(8 * 1024 * 1024 + 1)], 'recent-logs.txt');
    const { form } = bugReportForm({ message: 'Huge log attached.' }, oversized);

    const res = await SELF.fetch(`${BASE}/bug-report`, { method: 'POST', body: form });

    expect(res.status).toBe(400);
  });

  it('rejects a request without a message', async () => {
    const { form } = bugReportForm({});
    const res = await SELF.fetch(`${BASE}/bug-report`, { method: 'POST', body: form });

    expect(res.status).toBe(400);
  });
});
