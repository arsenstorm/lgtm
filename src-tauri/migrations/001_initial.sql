CREATE TABLE repositories (
  id TEXT PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  remote_url TEXT,
  default_base_branch TEXT,
  last_opened_at TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE review_sessions (
  id TEXT PRIMARY KEY,
  repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
  source_kind TEXT NOT NULL CHECK (source_kind IN ('working-tree','branch','github-pull-request')),
  base_revision TEXT,
  head_revision TEXT,
  base_sha TEXT,
  head_sha TEXT,
  status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','closed')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_review_sessions_repo ON review_sessions(repository_id, status);

CREATE TABLE review_comments (
  id TEXT PRIMARY KEY,
  review_session_id TEXT NOT NULL REFERENCES review_sessions(id) ON DELETE CASCADE,
  file_path TEXT NOT NULL,
  side TEXT NOT NULL CHECK (side IN ('old','new')),
  start_line INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  body TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','published','outdated','deleted')),
  language TEXT,
  selected_code TEXT NOT NULL,
  context_before TEXT NOT NULL,
  context_after TEXT NOT NULL,
  context_hash TEXT NOT NULL,
  hunk_header TEXT NOT NULL,
  base_revision TEXT,
  head_revision TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_review_comments_session ON review_comments(review_session_id, status);
CREATE INDEX idx_review_comments_file ON review_comments(review_session_id, file_path);

CREATE TABLE memory_examples (
  id TEXT PRIMARY KEY,
  source_comment_id TEXT REFERENCES review_comments(id) ON DELETE SET NULL,
  repository_id TEXT REFERENCES repositories(id) ON DELETE CASCADE,
  scope TEXT NOT NULL DEFAULT 'repository' CHECK (scope IN ('repository','global')),
  language TEXT,
  comment_body TEXT NOT NULL,
  selected_code TEXT NOT NULL,
  context_before TEXT NOT NULL DEFAULT '',
  context_after TEXT NOT NULL DEFAULT '',
  file_path TEXT NOT NULL,
  normalized_code TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  positive_feedback INTEGER NOT NULL DEFAULT 0,
  negative_feedback INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_memory_examples_lookup ON memory_examples(enabled, language, repository_id);

CREATE TABLE suggestions (
  id TEXT PRIMARY KEY,
  review_session_id TEXT NOT NULL REFERENCES review_sessions(id) ON DELETE CASCADE,
  memory_example_id TEXT NOT NULL REFERENCES memory_examples(id) ON DELETE CASCADE,
  file_path TEXT NOT NULL,
  anchor_json TEXT NOT NULL,
  proposed_body TEXT NOT NULL,
  similarity_score REAL NOT NULL,
  adjusted_confidence REAL NOT NULL,
  status TEXT NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed','accepted','accepted-after-edit','dismissed','suppressed')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_suggestions_session ON suggestions(review_session_id, status);
CREATE INDEX idx_suggestions_memory ON suggestions(memory_example_id, status);

CREATE TABLE file_review_state (
  review_session_id TEXT NOT NULL REFERENCES review_sessions(id) ON DELETE CASCADE,
  file_path TEXT NOT NULL,
  viewed INTEGER NOT NULL DEFAULT 0,
  last_viewed_at TEXT,
  PRIMARY KEY (review_session_id, file_path)
);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
