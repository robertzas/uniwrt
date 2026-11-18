-- Node backups kept by count and content (features/lifecycle.md, Backups; D-126). `content` is a
-- digest of the files the tarball holds, whatever their times (NULL when it can't be read): a
-- backup of the same files as the node's newest only sets that one's `seen_at`.
ALTER TABLE node_backups ADD COLUMN content TEXT;
ALTER TABLE node_backups ADD COLUMN seen_at INTEGER;
