-- The node inventory and the latest facts snapshot of each node.
-- Timestamps are Unix milliseconds. Enum-like columns hold serde names and are validated when
-- read, so a new value needs no table rebuild.

-- Adopted nodes. Identity is the label (or primary) MAC. AUTOINCREMENT keeps ids from being
-- reused, so history that outlives a deleted node never points at a newer one.
CREATE TABLE nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    mac TEXT NOT NULL UNIQUE,
    board_name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    role TEXT NOT NULL,
    mode TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

-- The latest NodeFacts (JSON) of each node, from which the engine restores live state.
CREATE TABLE node_facts (
    node_id INTEGER PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    collected_at INTEGER NOT NULL,
    facts TEXT NOT NULL
) STRICT;
