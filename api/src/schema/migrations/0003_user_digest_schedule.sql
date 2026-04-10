ALTER TABLE users
  ADD COLUMN email_digest_minutes_utc INTEGER NOT NULL DEFAULT 360;
