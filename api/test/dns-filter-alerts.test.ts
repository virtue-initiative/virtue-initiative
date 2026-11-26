import { beforeEach, describe, expect, it } from 'vitest';
import { SELF } from 'cloudflare:test';
import {
  acceptPartnerInvite,
  authHeaders,
  BASE,
  clearDB,
  createDeviceForUser,
  invitePartner,
  listEmailDeliveries,
  markUserEmailVerified,
  signupAndGetCookie,
} from './helpers';

beforeEach(clearDB);

const KIND = 'dns_filter_check_changed';

async function ownerWithPartner(ownerEmail: string, partnerEmail: string) {
  const { cookie } = await signupAndGetCookie(ownerEmail, 'password123', 'Sam');
  const partner = await signupAndGetCookie(partnerEmail);
  await markUserEmailVerified(partner.userId);
  const { token } = await invitePartner(cookie, partnerEmail);
  expect((await acceptPartnerInvite(partner.cookie, token)).status).toBe(200);
  return { cookie, partnerCookie: partner.cookie };
}

async function patchUser(cookie: string, body: Record<string, unknown>) {
  const res = await SELF.fetch(`${BASE}/user`, {
    method: 'PATCH',
    headers: authHeaders(cookie),
    body: JSON.stringify(body),
  });
  expect(res.status).toBe(200);
}

async function patchDevice(cookie: string, id: string, body: Record<string, unknown>) {
  const res = await SELF.fetch(`${BASE}/device/${id}`, {
    method: 'PATCH',
    headers: authHeaders(cookie),
    body: JSON.stringify(body),
  });
  expect(res.status).toBe(204);
}

async function alerts() {
  return (await listEmailDeliveries()).filter((delivery) => delivery.kind === KIND);
}

describe('DNS filter check alerts on PATCH /user (API-020)', () => {
  it('emails the partner at once when the check domain is cleared, naming it (API-020)', async () => {
    const { cookie } = await ownerWithPartner('dns-off@example.com', 'dns-off-partner@example.com');
    await patchUser(cookie, { dns_filter_domain: 'blocked.example.net' });
    expect(await alerts()).toHaveLength(0);

    await patchUser(cookie, { dns_filter_domain: null });

    const sent = await alerts();
    expect(sent).toHaveLength(1);
    expect(sent[0]?.recipient_email).toBe('dns-off-partner@example.com');
    expect(sent[0]?.text).toContain('blocked.example.net');
  });

  it('emails the partner when the check domain changes, naming both domains (API-020)', async () => {
    const { cookie } = await ownerWithPartner(
      'dns-change@example.com',
      'dns-change-partner@example.com',
    );
    await patchUser(cookie, { dns_filter_domain: 'old.example.net' });

    await patchUser(cookie, { dns_filter_domain: 'new.example.org' });

    const sent = await alerts();
    expect(sent).toHaveLength(1);
    expect(sent[0]?.text).toContain('old.example.net');
    expect(sent[0]?.text).toContain('new.example.org');
  });

  it('reports turning the check off and on again, which a device could miss (API-020)', async () => {
    const { cookie } = await ownerWithPartner(
      'dns-flip@example.com',
      'dns-flip-partner@example.com',
    );
    await patchUser(cookie, { dns_filter_domain: 'blocked.example.net' });

    await patchUser(cookie, { dns_filter_domain: null });
    await patchUser(cookie, { dns_filter_domain: 'blocked.example.net' });

    expect(await alerts()).toHaveLength(1);
  });

  it('does not email when a domain is first set, resent unchanged, or left out (API-020)', async () => {
    const { cookie } = await ownerWithPartner(
      'dns-quiet@example.com',
      'dns-quiet-partner@example.com',
    );

    await patchUser(cookie, { dns_filter_domain: 'blocked.example.net' });
    await patchUser(cookie, { dns_filter_domain: ' Blocked.Example.NET ' });
    await patchUser(cookie, { name: 'Renamed' });

    expect(await alerts()).toHaveLength(0);
  });

  it('skips partners whose email_frequency is none (API-020)', async () => {
    const { cookie, partnerCookie } = await ownerWithPartner(
      'dns-none@example.com',
      'dns-none-partner@example.com',
    );
    await patchUser(partnerCookie, { settings: { email_frequency: 'none' } });
    await patchUser(cookie, { dns_filter_domain: 'blocked.example.net' });

    await patchUser(cookie, { dns_filter_domain: null });

    expect(await alerts()).toHaveLength(0);
  });
});

describe('DNS filter check alerts on PATCH /device/:id (API-030)', () => {
  it('emails the partner at once when the check is turned off for a device, naming it (API-030)', async () => {
    const { cookie } = await ownerWithPartner('dev-off@example.com', 'dev-off-partner@example.com');
    await patchUser(cookie, { dns_filter_domain: 'blocked.example.net' });
    const device = await createDeviceForUser('dev-off@example.com', 'password123', 'Desk PC');

    await patchDevice(cookie, device.id, { dns_filter_disabled: true });

    const sent = await alerts();
    expect(sent).toHaveLength(1);
    expect(sent[0]?.recipient_email).toBe('dev-off-partner@example.com');
    expect(sent[0]?.text).toContain('Desk PC');
  });

  it('does not email when turning it back on, resending the value, or with no account domain (API-030)', async () => {
    const { cookie } = await ownerWithPartner(
      'dev-quiet@example.com',
      'dev-quiet-partner@example.com',
    );
    const device = await createDeviceForUser('dev-quiet@example.com', 'password123', 'Desk PC');

    await patchDevice(cookie, device.id, { dns_filter_disabled: true });
    expect(await alerts()).toHaveLength(0);

    await patchUser(cookie, { dns_filter_domain: 'blocked.example.net' });
    await patchDevice(cookie, device.id, { dns_filter_disabled: true });
    await patchDevice(cookie, device.id, { dns_filter_disabled: false });
    await patchDevice(cookie, device.id, { name: 'Renamed' });

    expect(await alerts()).toHaveLength(0);
  });
});
