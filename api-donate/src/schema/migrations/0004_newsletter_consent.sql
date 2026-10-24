-- Consent state for the newsletter, which we send ourselves through SES. A row only receives
-- mail when confirmed_at is set and neither unsubscribed_at nor suppressed_at is. Existing rows
-- are left unconfirmed.
ALTER TABLE newsletter_subscribers ADD COLUMN confirmed_at INTEGER;          -- double opt-in click, or a verified account signup
ALTER TABLE newsletter_subscribers ADD COLUMN unsubscribed_at INTEGER;       -- the subscriber opted out; confirming or opting in again clears it
ALTER TABLE newsletter_subscribers ADD COLUMN suppressed_at INTEGER;         -- hard bounce or complaint; nothing clears it, and we never email it again
ALTER TABLE newsletter_subscribers ADD COLUMN confirmation_sent_at INTEGER;  -- when the latest confirmation email went out; throttles resends and ties the link to that email
