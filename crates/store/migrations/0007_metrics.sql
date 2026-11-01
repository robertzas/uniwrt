-- Metrics (features/observability.md, Metrics): series named by entity and metric
-- (uniwrt_model::metric), each with raw 15-second windows (48 h), 1-minute rollups (14 d) and
-- 1-hour rollups (400 d). Times are Unix milliseconds at the start of each window or bucket.
CREATE TABLE metric_series (
    id INTEGER PRIMARY KEY,
    entity TEXT NOT NULL,
    metric TEXT NOT NULL,
    UNIQUE (entity, metric)
) STRICT;

CREATE TABLE metrics_raw (
    series INTEGER NOT NULL,
    ts INTEGER NOT NULL,
    value REAL NOT NULL,
    PRIMARY KEY (series, ts)
) STRICT, WITHOUT ROWID;

-- A rollup keeps the least, mean, greatest and last value of its bucket and how many raw
-- windows it covers (`n`), which weighs its mean in the next rollup.
CREATE TABLE metrics_1m (
    series INTEGER NOT NULL,
    ts INTEGER NOT NULL,
    low REAL NOT NULL,
    mean REAL NOT NULL,
    high REAL NOT NULL,
    last REAL NOT NULL,
    n INTEGER NOT NULL,
    PRIMARY KEY (series, ts)
) STRICT, WITHOUT ROWID;

CREATE TABLE metrics_1h (
    series INTEGER NOT NULL,
    ts INTEGER NOT NULL,
    low REAL NOT NULL,
    mean REAL NOT NULL,
    high REAL NOT NULL,
    last REAL NOT NULL,
    n INTEGER NOT NULL,
    PRIMARY KEY (series, ts)
) STRICT, WITHOUT ROWID;

-- How far each rollup got: every bucket before `done_until` is rolled up.
CREATE TABLE metric_rollups (
    resolution TEXT PRIMARY KEY,
    done_until INTEGER NOT NULL
) STRICT;
