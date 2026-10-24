-- Consent state for the newsletter, which we send ourselves through SES. A row only receives
-- mail when confirmed_at is set and unsubscribed_at is not. Existing rows are left unconfirmed.
ALTER TABLE newsletter_subscribers ADD COLUMN confirmed_at INTEGER;          -- double opt-in click, or a verified account signup
ALTER TABLE newsletter_subscribers ADD COLUMN unsubscribed_at INTEGER;       -- cleared again when the address re-confirms
ALTER TABLE newsletter_subscribers ADD COLUMN confirmation_sent_at INTEGER;  -- throttles repeat confirmation emails
