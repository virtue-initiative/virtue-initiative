-- api/SPEC.md API-030 / CORE-027: the domain the owner blocks in their DNS filter,
-- or NULL when the check is off.
ALTER TABLE devices ADD COLUMN dns_filter_domain TEXT;
