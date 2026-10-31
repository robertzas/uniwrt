-- The topology's history, for the replay (features/observability.md, Topology): keyframes (a
-- whole uniwrt_topo::Topology as JSON) and the deltas between them (uniwrt_topo::TopologyDelta),
-- in the order they were recorded. The engine encodes them; the store keeps them opaque.
CREATE TABLE topology_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at INTEGER NOT NULL,
    keyframe INTEGER NOT NULL,
    data TEXT NOT NULL
) STRICT;
CREATE INDEX topology_history_at ON topology_history (at);
CREATE INDEX topology_keyframes ON topology_history (at) WHERE keyframe = 1;

-- Where users pinned vertices of the topology view, in its world coordinates, by vertex id
-- (`node:3`, `client:<mac>`): one layout for every user and browser.
CREATE TABLE topology_layout (
    vertex TEXT PRIMARY KEY,
    x REAL NOT NULL,
    y REAL NOT NULL
) STRICT;
