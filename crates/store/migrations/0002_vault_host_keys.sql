-- The secrets vault and the host key pinned for each node.

-- Secrets by name, such as the controller's SSH key ("controller/ssh_key"). `alg` says how
-- `value` is sealed: 'plain' (the M1.2 stub, unencrypted) until the vault seals every value with
-- XChaCha20-Poly1305 (M1.6, features/security.md), which reseals 'plain' rows when it opens.
CREATE TABLE vault (
    name TEXT PRIMARY KEY,
    alg TEXT NOT NULL,
    value BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

-- The SSH host key each node must present (trust on first use, confirmed by the user at
-- adoption). A mismatch quarantines the node until an admin re-trusts it.
CREATE TABLE node_host_keys (
    node_id INTEGER PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    public_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    pinned_at INTEGER NOT NULL
) STRICT;
