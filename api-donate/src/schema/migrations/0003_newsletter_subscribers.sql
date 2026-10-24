-- Where each newsletter subscriber signed up from. Buttondown stores no metadata for us, so
-- country and region come from Cloudflare's geoip on the signup request.
CREATE TABLE newsletter_subscribers (
  email TEXT PRIMARY KEY,
  country TEXT,                        -- ISO 3166-1 alpha-2, e.g. 'US'
  region_code TEXT,                    -- ISO 3166-2 subdivision code, e.g. 'CA' for California
  region TEXT,                         -- subdivision name, e.g. 'California'
  source TEXT NOT NULL,                -- 'landing' (newsletter form) or 'signup' (account signup)
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
