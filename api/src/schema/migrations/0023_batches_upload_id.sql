-- api/SPEC.md API-038: the client's id for a batch, kept across retries, so a
-- retry of a batch whose response was lost returns the stored batch instead
-- of storing it again. Nullable, for rows from older clients. No index: it is
-- only read on a retry, through the owner's primary key range, and an index
-- would cost a written row on every upload.
ALTER TABLE batches ADD COLUMN upload_id BLOB;
