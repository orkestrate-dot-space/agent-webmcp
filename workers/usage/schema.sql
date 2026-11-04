-- One row per domain per UTC hour. The count is the number of
-- get_agent skill requests observed by the directory. No identity,
-- no query text, no IP — just domain + hour + count.
CREATE TABLE IF NOT EXISTS usage (
  domain TEXT NOT NULL,
  bucket TEXT NOT NULL, -- YYYY-MM-DDTHH (UTC)
  count  INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (domain, bucket)
) WITHOUT ROWID;
