-- Notifiers (features/observability.md, Notifications; uniwrt_model::notify).
--
-- `notifiers`: each notifier's spec and filter as JSON, without its secrets (the vault's
-- `notifier/<id>`), with how its last deliveries went.
CREATE TABLE notifiers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    spec TEXT NOT NULL,
    filter TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_ok_at INTEGER,
    last_error_at INTEGER,
    last_error TEXT
) STRICT;

-- `settings`: the controller's settings by key, each a JSON value.
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
