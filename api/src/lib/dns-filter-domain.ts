// API-020: a DNS check domain must serve a website over a valid certificate.
// A device counts a domain without one as blocked (CORE-029), so a check on
// such a domain would never alert.

const TIMEOUT_MS = 5_000;

/**
 * Whether `https://<domain>/` answers within 5 seconds over a valid
 * certificate. Any HTTP status counts, and redirects are not followed. A
 * failed lookup, connection or certificate makes `fetch` throw. Cloudflare's
 * 52x statuses mean its edge could not reach the origin or trust its
 * certificate, so they do not count as an answer.
 */
export async function servesHttps(domain: string): Promise<boolean> {
  try {
    const res = await fetch(`https://${domain}/`, {
      method: 'HEAD',
      redirect: 'manual',
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    if (res.status >= 520 && res.status <= 530) {
      console.warn(`[dns-filter] https://${domain}/ answered ${res.status} from Cloudflare's edge`);
      return false;
    }
    return true;
  } catch (err) {
    console.warn(`[dns-filter] https://${domain}/ did not answer`, err);
    return false;
  }
}
