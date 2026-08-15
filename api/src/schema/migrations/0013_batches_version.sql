-- api/SPEC.md: batches now record the API's current major version alongside
-- the encrypted blob, so future versions of the wire format can be told apart.
-- Existing rows predate versioning, so they default to an empty string rather
-- than a guessed version.
ALTER TABLE batches ADD COLUMN version TEXT NOT NULL DEFAULT '';
