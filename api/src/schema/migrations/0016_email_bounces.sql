-- API-056: one row per bounced recipient reported by SES. `reason` is
-- 'permanent' or 'temporary'. An address with a permanent bounce is never
-- sent to again.
CREATE TABLE email_bounces (
  id BLOB PRIMARY KEY,
  email TEXT NOT NULL,
  bounced_at INTEGER NOT NULL,
  reason TEXT NOT NULL
);

CREATE INDEX idx_email_bounces_email ON email_bounces(email);

-- The bounce that stopped this token's email from being delivered, if any.
ALTER TABLE email_tokens ADD COLUMN bounce_id BLOB REFERENCES email_bounces(id) ON DELETE SET NULL;
