-- Jobs, and what adoption keeps about a node besides its facts: its packages and its
-- configuration backups (features/adoption.md, Path A). Audit entries gain a detail.

-- What an audited action was about, for people, such as the node an adoption targets.
ALTER TABLE audit ADD COLUMN detail TEXT;

-- Jobs as history (D-042): a job runs in memory and is saved at each step. One still `running`
-- when the controller starts was interrupted, and is marked failed. `steps` is a JSON array of
-- model::job::Step and `error` a JSON model::job::JobError. The node and the user are kept as
-- plain ids, like the audit log's, so history outlives them.
CREATE TABLE jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    target TEXT NOT NULL,
    node_id INTEGER,
    created_by INTEGER,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    steps TEXT NOT NULL,
    error TEXT
) STRICT;
CREATE INDEX jobs_running ON jobs (state) WHERE state = 'running';

-- The packages each node runs (a JSON array of model::inventory::Package), as read at adoption.
CREATE TABLE node_inventory (
    node_id INTEGER PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    taken_at INTEGER NOT NULL,
    packages TEXT NOT NULL
) STRICT;

-- Configuration backups (`sysupgrade -b` tarballs). They hold password hashes, private host keys
-- and Wi-Fi keys, so each is sealed like a vault secret (`alg`), named `backup/<sha256>` as its
-- associated data; `sha256` and `size` describe the tarball itself.
CREATE TABLE node_backups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    taken_at INTEGER NOT NULL,
    reason TEXT NOT NULL,
    size INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    alg TEXT NOT NULL,
    value BLOB NOT NULL
) STRICT;
CREATE INDEX node_backups_by_node ON node_backups (node_id, taken_at);
