ALTER TABLE users ADD COLUMN settings TEXT NOT NULL DEFAULT '{}';

UPDATE users SET settings = json_object(
  'email_frequency', COALESCE(email_frequency, 'daily'),
  'timezone', 'UTC'
);

DROP INDEX IF EXISTS idx_users_email_frequency;
ALTER TABLE users DROP COLUMN email_frequency;
ALTER TABLE users DROP COLUMN email_digest_minutes_utc;
