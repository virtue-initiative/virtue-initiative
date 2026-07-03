-- listDeviceLogs has the same shape as listBatches: it filters
-- `user_id IN (...) AND created_at > ?` and orders by created_at. With only
-- single-column indexes on user_id and created_at, the planner seeks all of
-- a user's logs across all time, then filters by created_at and sorts in a
-- temp b-tree. A composite index lets it seek directly to the requested
-- range and satisfies the ORDER BY for free.
CREATE INDEX idx_device_logs_user_id_created_at ON device_logs(user_id, created_at);

-- Superseded by the composite index above (same leftmost prefix covers
-- every existing user_id-only lookup, e.g. listDeviceLogsForUser,
-- listRiskDeviceLogsForUser, findDeviceLogByKindWithinWindow).
DROP INDEX idx_device_logs_user_id;
