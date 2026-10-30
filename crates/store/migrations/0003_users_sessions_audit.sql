-- Controller users, their login sessions and the audit log (features/security.md).

-- Users. `username` is lowercase; `password` is an Argon2id PHC string.
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password TEXT NOT NULL,
    role TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

-- Login sessions, by the SHA-256 of their token: a copy of the database cannot be used to log
-- in. A session ends at `expires_at`, or earlier when idle (the API decides how long).
CREATE TABLE sessions (
    token_sha256 BLOB PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    ip TEXT,
    user_agent TEXT
) STRICT, WITHOUT ROWID;
CREATE INDEX sessions_by_user ON sessions (user_id);

-- The audit log, oldest first. `user_id` is kept when the user is deleted: the entry still
-- names them.
CREATE TABLE audit (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at INTEGER NOT NULL,
    action TEXT NOT NULL,
    user_id INTEGER,
    username TEXT,
    ip TEXT
) STRICT;
