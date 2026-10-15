-- API-035/API-057: the App Store app transaction ID an iOS device registered
-- with, so a RESCIND_CONSENT notification can find that download's devices.
ALTER TABLE devices ADD COLUMN app_transaction_id TEXT;

CREATE INDEX idx_devices_app_transaction_id ON devices(app_transaction_id);
