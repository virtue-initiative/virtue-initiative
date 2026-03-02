-- Track the device state at the start of each batch window.
-- Set to the same random bytes as state when a batch is uploaded (batch boundary reset).
-- Allows the server to record start_chain_hash and end_chain_hash without client input.
ALTER TABLE device_states ADD COLUMN batch_start_state BLOB;
