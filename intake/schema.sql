-- Intake state. One row per request (site listing or takedown); the index
-- itself lives in KV, which is the published copy clients read.
CREATE TABLE IF NOT EXISTS requests (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,                -- site | takedown | rebuild
  host TEXT NOT NULL,
  entry TEXT NOT NULL,               -- host, or host/path when under one
  pages TEXT NOT NULL DEFAULT '[]',  -- JSON array of extra URLs
  state TEXT NOT NULL,               -- queued | scanning | done | failed
  outcome TEXT,                      -- listed | no_tools | blocked | error | taken_down | rebuilt
  message TEXT,
  tools INTEGER NOT NULL DEFAULT 0,
  token TEXT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  ip_hash TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  checked_at TEXT
);
CREATE INDEX IF NOT EXISTS requests_state ON requests(state, created_at);
CREATE INDEX IF NOT EXISTS requests_host ON requests(host, created_at);
