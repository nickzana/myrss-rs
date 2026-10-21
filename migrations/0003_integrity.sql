CREATE TRIGGER IF NOT EXISTS entries_fts_delete
AFTER DELETE ON entries
BEGIN
  DELETE FROM entries_fts WHERE rowid = old.id;
END;
