-- Newsletter mailing list. The main api worker writes rows through its DONATE_DB binding
-- (api/SPEC.md API-010, API-058 to API-060), and scripts/newsletter.ts reads them to send.
-- Only rows that are confirmed and not unsubscribed get newsletters.
CREATE TABLE newsletter_signups (
  email TEXT PRIMARY KEY,              -- lowercased
  token TEXT NOT NULL UNIQUE DEFAULT (lower(hex(randomblob(16)))), -- in confirm and unsubscribe links
  country TEXT,                        -- ISO 3166-1 alpha-2, from Cloudflare's request.cf.country
  region TEXT,                         -- ISO 3166-2 subdivision code (a US state), request.cf.regionCode
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  confirmed_at INTEGER,
  confirmation_sent_at INTEGER,        -- last confirmation email, to limit resends
  unsubscribed_at INTEGER              -- unsubscribe link, a hard bounce or complaint, or the CLI
);
