-- Drift (features/lifecycle.md, Drift; D-072, D-074). Each managed package as the controller
-- confirmed it on a node: the file the node held once an apply confirmed it (or found it
-- unchanged), which the agent's hashes are compared with. An apply replaces a node's rows.
CREATE TABLE node_config (
    node_id INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    package TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    text TEXT NOT NULL,
    revision_id INTEGER NOT NULL REFERENCES revisions (id),
    confirmed_at INTEGER NOT NULL,
    PRIMARY KEY (node_id, package)
) STRICT;

-- The packages that differ from what was confirmed: what the node holds now (NULL once the
-- file is gone), and whether a user accepted it until it changes again.
CREATE TABLE config_drift (
    node_id INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    package TEXT NOT NULL,
    sha256 TEXT,
    text TEXT,
    detected_at INTEGER NOT NULL,
    changed_at INTEGER NOT NULL,
    acknowledged_at INTEGER,
    acknowledged_by TEXT,
    PRIMARY KEY (node_id, package)
) STRICT;

-- The nodes' packages whose drift isn't watched.
CREATE TABLE drift_mutes (
    node_id INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    package TEXT NOT NULL,
    muted_at INTEGER NOT NULL,
    muted_by TEXT,
    PRIMARY KEY (node_id, package)
) STRICT;
