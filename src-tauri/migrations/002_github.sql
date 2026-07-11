ALTER TABLE review_sessions ADD COLUMN pull_number INTEGER;

CREATE TABLE imported_github_comments (
  id TEXT PRIMARY KEY,
  repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
  pull_number INTEGER NOT NULL,
  path TEXT NOT NULL,
  body TEXT NOT NULL,
  diff_hunk TEXT NOT NULL DEFAULT '',
  original_line INTEGER,
  side TEXT,
  author_login TEXT NOT NULL,
  commented_at TEXT NOT NULL,
  imported_at TEXT NOT NULL
);
CREATE INDEX idx_imported_github_comments_repo ON imported_github_comments(repository_id);
