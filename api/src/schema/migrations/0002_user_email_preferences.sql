ALTER TABLE users
  ADD COLUMN email_frequency TEXT NOT NULL DEFAULT 'daily'
  CHECK (email_frequency IN ('none', 'alerts-only', 'daily', 'weekly'));

CREATE INDEX idx_users_email_frequency ON users(email_frequency);

DROP TABLE IF EXISTS partner_preferences;
