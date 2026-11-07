-- Every index is one more row written on each insert into its table, and on
-- each update that touches its column. These three cost writes without making
-- any query cheaper.

-- `email TEXT UNIQUE` already maintains an index on users(email), and that is
-- the one the planner uses for email lookups.
DROP INDEX idx_users_email;

-- status has three values, so this index selects a large share of the table.
-- Worse, without table statistics the planner prefers it over the far more
-- selective idx_partners_watching_user_id / idx_partners_watcher_user_id, so
-- "this user's accepted partners" walks every accepted partnership. Only the
-- analytics counts filter on status alone, and they read most of the table
-- either way.
DROP INDEX idx_partners_status;

-- Same shape: purpose has a handful of values, and every query that filters on
-- it also has the token hash or the user id. The planner was picking this index
-- over idx_email_tokens_user_id when consuming a user's outstanding tokens.
DROP INDEX idx_email_tokens_purpose;
