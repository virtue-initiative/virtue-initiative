ALTER TABLE partners ADD COLUMN accepted_at INTEGER;

UPDATE partners SET accepted_at = updated_at WHERE status = 'accepted';
