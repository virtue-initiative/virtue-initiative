-- api/SPEC.md API-020 / API-030 / CORE-028: the domain the user blocks in their DNS
-- filter, or NULL when none is set, and a per-device switch that turns the check off.
ALTER TABLE users ADD COLUMN dns_filter_domain TEXT;
ALTER TABLE devices ADD COLUMN dns_filter_disabled INTEGER NOT NULL DEFAULT 0;
