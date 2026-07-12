-- Virtue Initiative donations schema

CREATE TABLE donations (
  id TEXT PRIMARY KEY,                 -- Stripe Checkout session id
  stripe_customer_id TEXT,
  stripe_subscription_id TEXT,
  email TEXT,
  name TEXT,
  phone TEXT,
  amount_cents INTEGER NOT NULL,
  currency TEXT NOT NULL DEFAULT 'usd',
  recurring INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX idx_donations_email ON donations(email);
CREATE INDEX idx_donations_subscription ON donations(stripe_subscription_id);

-- Processed Stripe event ids, used to make webhook handling idempotent.
CREATE TABLE stripe_events (
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
