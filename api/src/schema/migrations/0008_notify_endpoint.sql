-- #467: Replace the direct-log endpoint with a notification-only endpoint.
--
-- High-risk events now flow through the end-to-end encrypted batch pipeline
-- instead of being stored unencrypted via POST /d/log. Batches carry a count of
-- the high- and medium-risk events they contain so digest emails can still
-- summarize tamper activity without reading the encrypted payload.

ALTER TABLE batches ADD COLUMN high_risk_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE batches ADD COLUMN medium_risk_count INTEGER NOT NULL DEFAULT 0;

-- The device_logs table stored unencrypted event bodies; it is no longer written
-- to or read from now that high-risk events live in encrypted batches.
DROP TABLE IF EXISTS device_logs;
