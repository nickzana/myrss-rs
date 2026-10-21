PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS deleted_entries (
  feed_id INTEGER NOT NULL,
  guid TEXT NOT NULL,
  deleted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY(feed_id,guid)
);
