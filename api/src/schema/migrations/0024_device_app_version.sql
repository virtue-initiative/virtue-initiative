-- api/SPEC.md API-035, API-036: the client version a device last reported, so
-- operators can see whether updates are reaching devices. NULL until a client
-- that sends X-Virtue-Version registers or fetches its settings.
ALTER TABLE devices ADD COLUMN app_version TEXT;
