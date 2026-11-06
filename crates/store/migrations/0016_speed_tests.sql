-- Speed tests from the gateway (features/wan.md, SQM; D-076): the history the Internet page
-- charts and autotune compares with. One row per job; the newest 500 are kept.
CREATE TABLE speed_tests (
    job_id INTEGER PRIMARY KEY REFERENCES jobs (id) ON DELETE CASCADE,
    node_id INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    at INTEGER NOT NULL,
    bypassed INTEGER NOT NULL,
    download_kbps INTEGER NOT NULL,
    upload_kbps INTEGER NOT NULL,
    idle_us INTEGER NOT NULL,
    download_us INTEGER NOT NULL,
    upload_us INTEGER NOT NULL,
    grade TEXT NOT NULL
) STRICT;

CREATE INDEX speed_tests_at ON speed_tests (at);
