-- idx_devices_id duplicates the automatic index SQLite already maintains for
-- the `id BLOB PRIMARY KEY` column -- pure write-cost overhead with zero
-- query benefit.
DROP INDEX idx_devices_id;

-- batches.device_id is filtered on every device delete (listBatchUrlsForDevice,
-- and the DELETE FROM batches cleanup) but has no index today, forcing a full
-- scan that grows with the table.
CREATE INDEX idx_batches_device_id ON batches(device_id);

-- user_sessions and device_sessions grow unboundedly with no cleanup job.
-- Index expires_at so a chunked prune-by-expiry delete doesn't itself scan
-- the full table. (email_tokens already has idx_email_tokens_expires_at from
-- 0001_schema.sql.)
CREATE INDEX idx_user_sessions_expires_at ON user_sessions(expires_at);
CREATE INDEX idx_device_sessions_expires_at ON device_sessions(expires_at);
