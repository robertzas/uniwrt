-- Clients (features/observability.md, Clients): what the controller keeps about each device
-- that connects through the nodes, by MAC address, and the alias a user gave it.
CREATE TABLE clients (
    mac TEXT PRIMARY KEY,
    alias TEXT,
    hostname TEXT,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL
) STRICT;
CREATE INDEX clients_last_seen ON clients (last_seen);

-- Where each client was connected and when (uniwrt_model::client::ClientSession): an access
-- point's interface or a node's port. A client's sessions don't overlap, so its MAC and the
-- session's start identify one. `node` is not a foreign key: the history outlives a node.
CREATE TABLE client_sessions (
    mac TEXT NOT NULL,
    started INTEGER NOT NULL,
    ended INTEGER,
    ended_by TEXT,
    node INTEGER,
    port TEXT,
    medium TEXT NOT NULL,
    band TEXT,
    ssid TEXT,
    PRIMARY KEY (mac, started)
) STRICT, WITHOUT ROWID;
CREATE INDEX client_sessions_ended ON client_sessions (ended);
