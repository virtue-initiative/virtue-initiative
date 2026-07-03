-- listBatches filters batches by `user_id IN (...) AND created_at > ?` and
-- orders by created_at. With only single-column indexes on user_id and
-- created_at, the planner seeks all of a user's batches across all time
-- (idx_batches_user_id), then filters by created_at and sorts in a temp
-- b-tree -- reading far more rows than the time-bounded result set needs.
-- A composite index lets it seek directly to the requested range and
-- satisfies the ORDER BY for free, with no temp b-tree.
CREATE INDEX idx_batches_user_id_created_at ON batches(user_id, created_at);

-- Superseded by the composite index above (same leftmost prefix covers
-- every existing user_id-only lookup).
DROP INDEX idx_batches_user_id;
