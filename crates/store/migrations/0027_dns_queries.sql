-- DNS query statistics (features/network.md, Query statistics; D-133): the gateway's queries of
-- the last 24 h, and 30 days of hourly counts per client (its MAC, or its address where the
-- controller doesn't know who holds it). `outcome` is model::dns_queries::Outcome's serde name.
CREATE TABLE dns_queries (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    client TEXT NOT NULL,
    mac TEXT,
    qtype TEXT NOT NULL,
    domain TEXT NOT NULL,
    outcome TEXT NOT NULL
) STRICT;
CREATE INDEX dns_queries_at ON dns_queries (at);
CREATE TABLE dns_hours (
    hour INTEGER NOT NULL,
    client TEXT NOT NULL,
    queries INTEGER NOT NULL,
    blocked INTEGER NOT NULL,
    PRIMARY KEY (hour, client)
) STRICT, WITHOUT ROWID;
