-- Zero-touch provisioning (features/adoption.md, Path B; D-102): the pending adoption inbox, one
-- row per device by its MAC with its latest hello, and the owner's single-use pre-approvals by
-- MAC. The job is a plain id, so a row outlives job history.
CREATE TABLE ztp_pending (
    mac TEXT PRIMARY KEY,
    board_name TEXT NOT NULL,
    model TEXT NOT NULL,
    hostname TEXT NOT NULL,
    release TEXT NOT NULL,
    role TEXT,
    address TEXT NOT NULL,
    host_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    job_id INTEGER
) STRICT;

CREATE TABLE ztp_approvals (
    mac TEXT PRIMARY KEY,
    name TEXT,
    role TEXT,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    created_by TEXT
) STRICT;
