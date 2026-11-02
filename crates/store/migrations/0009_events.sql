-- Events (features/observability.md, Events): the network's timeline (uniwrt_model::events).
-- `what` is the event's JSON, tagged by its `kind` (a column too, for filters); `level` is 0
-- critical, 1 warning, 2 notice, 3 info. `cond_key` names the condition it raises
-- (`cond_raised` 1) or clears (0). Ids grow in the order events were recorded. `node` is not a
-- foreign key: the history outlives a node.
CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at INTEGER NOT NULL,
    last_at INTEGER NOT NULL,
    count INTEGER NOT NULL,
    level INTEGER NOT NULL,
    category TEXT NOT NULL,
    kind TEXT NOT NULL,
    node INTEGER,
    client TEXT,
    cond_key TEXT,
    cond_raised INTEGER,
    what TEXT NOT NULL
) STRICT;
CREATE INDEX events_at ON events (at, id);
CREATE INDEX events_node_at ON events (node, at);
CREATE INDEX events_client_at ON events (client, at);
CREATE INDEX events_condition ON events (cond_key, id);
