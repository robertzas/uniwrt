-- Logs (features/observability.md, Logs): the entries of the nodes (read by the agent or
-- received by syslog) and of other hosts (by syslog). `time` is when it was logged, on the
-- sender's clock (or when it arrived, for a syslog message without a zone); `received` is the
-- controller's. Ids grow in arrival order, so retention deletes from the lowest. `node` is not
-- a foreign key: the history outlives a node. `via`: 0 agent, 1 syslog.
CREATE TABLE logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    time INTEGER NOT NULL,
    received INTEGER NOT NULL,
    node INTEGER,
    host TEXT,
    via INTEGER NOT NULL,
    seq INTEGER,
    facility INTEGER NOT NULL,
    severity INTEGER NOT NULL,
    program TEXT,
    pid INTEGER,
    message TEXT NOT NULL,
    pattern TEXT
) STRICT;
CREATE INDEX logs_time ON logs (time);
CREATE INDEX logs_node_time ON logs (node, time);

-- The full-text index of each entry's program and text, by the entry's id. Contentless: the
-- text is stored once, in `logs`, and index rows are deleted by id.
CREATE VIRTUAL TABLE logs_fts USING fts5 (program, message, content = '', contentless_delete = 1);
CREATE TRIGGER logs_fts_delete AFTER DELETE ON logs BEGIN
    DELETE FROM logs_fts WHERE rowid = old.id;
END;

-- The hosts and programs that logged, and when they last did, for filters.
CREATE TABLE log_sources (
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    last_seen INTEGER NOT NULL,
    PRIMARY KEY (kind, name)
) STRICT, WITHOUT ROWID;

-- Searches users saved (uniwrt_model::log::SavedSearch), shared by all; the filter is JSON.
CREATE TABLE log_searches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    filter TEXT NOT NULL,
    created_by INTEGER REFERENCES users (id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
) STRICT;
