-- Fundraising goals shown on the donate page.
CREATE TABLE goals (
  id TEXT PRIMARY KEY,                 -- stable slug, e.g. 'lawyer'
  title TEXT NOT NULL,
  description TEXT,
  target_cents INTEGER NOT NULL,
  raised_cents INTEGER NOT NULL DEFAULT 0, -- entered manually
  active INTEGER NOT NULL DEFAULT 1,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT INTO goals (id, title, description, target_cents, sort_order)
VALUES (
  'lawyer',
  'Legal fees',
  'Helps cover a lawyer to finish our nonprofit and 501(c)(3) paperwork.',
  100000,
  0
);
