-- Newsletter mailing list. The main api worker writes rows through its DONATE_DB binding
-- (api/SPEC.md API-010, API-058), and scripts/newsletter.ts reads them to send.
CREATE TABLE newsletter_signups (
  email TEXT PRIMARY KEY,              -- lowercased
  country TEXT,                        -- ISO 3166-1 alpha-2, from Cloudflare's request.cf.country
  region TEXT,                         -- ISO 3166-2 subdivision code (a US state), request.cf.regionCode
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  unsubscribed_at INTEGER              -- set by `bun run newsletter unsubscribe`; never mailed again
);
