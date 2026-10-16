-- listDevicesForOwners needs each device's latest batch end_time. With an index
-- on device_id alone it had to read every batch a device uploaded in the 30-day
-- retention window. Adding end_time lets MAX(end_time) be read straight off the
-- end of the device's index range: one row per device.
CREATE INDEX idx_batches_device_id_end_time ON batches(device_id, end_time);

-- Superseded by the composite index above (same leftmost prefix covers the
-- device_id-only lookups on device delete).
DROP INDEX idx_batches_device_id;
