-- last_seen_at and last_upload_at are now computed on fetch from
-- chain_hashes and r2_batches respectively.
ALTER TABLE devices DROP COLUMN last_seen_at;
ALTER TABLE devices DROP COLUMN last_upload_at;
