-- RF insight (features/wifi.md, RF insight; uniwrt_model::rf).
--
-- `rf_scans`: each radio's neighbor scans, kept 30 days: what they heard as JSON (`bsss` and
-- `channels`), with the number of networks for lists.
CREATE TABLE rf_scans (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    radio TEXT NOT NULL,
    band TEXT NOT NULL,
    at INTEGER NOT NULL,
    trigger TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    networks INTEGER NOT NULL,
    heard TEXT NOT NULL
) STRICT;

CREATE INDEX rf_scans_by_radio ON rf_scans (node_id, radio, at);
CREATE INDEX rf_scans_by_time ON rf_scans (at);
