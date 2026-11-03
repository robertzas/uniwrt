-- Alerts (features/observability.md, Alerts; uniwrt_model::alerts).
--
-- `alert_rules`: each rule's spec as JSON; `preset` names the built-in rule it is
-- (`alert-rules.toml`), unique, NULL for the user's own.
CREATE TABLE alert_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    preset TEXT UNIQUE,
    spec TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

-- `alerts`: the alert engine's state of each alert as JSON (`state`), with the columns filters
-- and the engine's writes need: the rule and subject (the latest alert of a pair is the one the
-- engine changes), level (0 critical … 3 info), node, when it started and resolved (NULL while
-- it fires). The acknowledgement is the users'. `rule_id` and `node` are not foreign keys: the
-- history outlives them.
CREATE TABLE alerts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id INTEGER NOT NULL,
    subject TEXT NOT NULL,
    level INTEGER NOT NULL,
    node INTEGER,
    started_at INTEGER NOT NULL,
    resolved_at INTEGER,
    state TEXT NOT NULL,
    ack_by INTEGER,
    ack_name TEXT,
    ack_at INTEGER
) STRICT;
CREATE INDEX alerts_key ON alerts (rule_id, subject, id);
CREATE INDEX alerts_started ON alerts (started_at, id);
CREATE INDEX alerts_active ON alerts (id) WHERE resolved_at IS NULL;
CREATE INDEX alerts_resolved ON alerts (resolved_at) WHERE resolved_at IS NOT NULL;

-- `maintenance_windows`: alerts on `nodes` (a JSON array; every node when empty) are silenced
-- from `starts_at` until `ends_at`.
CREATE TABLE maintenance_windows (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    starts_at INTEGER NOT NULL,
    ends_at INTEGER NOT NULL,
    nodes TEXT NOT NULL,
    note TEXT NOT NULL,
    created_by INTEGER,
    created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX maintenance_windows_ends ON maintenance_windows (ends_at);
