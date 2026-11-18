-- Rolling firmware upgrades (features/lifecycle.md, Firmware; D-125): a record that outlives a
-- controller restart, unlike a job (D-042). `nodes` is a JSON array of model::rollout::RolloutNode
-- in the order they upgrade; `canary`, `window`, `waiting` are JSON (or NULL) and `error` text.
-- The user is a plain id, like the jobs', so history outlives them.
CREATE TABLE rollouts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    state TEXT NOT NULL,
    nodes TEXT NOT NULL,
    canary TEXT,
    window TEXT,
    waiting TEXT,
    error TEXT,
    created_by INTEGER,
    started_at INTEGER NOT NULL,
    finished_at INTEGER
) STRICT;
CREATE INDEX rollouts_active ON rollouts (state) WHERE state IN ('running', 'paused', 'halted');
