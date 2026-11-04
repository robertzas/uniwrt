-- Revisions of the site's desired state (features/lifecycle.md, Desired state & revisions;
-- uniwrt_model::site; D-065): one linear history, every save a row, never changed.
--
-- `config` is the SiteConfig JSON in the schema it was written with (`schema`); reading one
-- upgrades it. Its secrets are references to the vault's `config/<id>` entries. `diff` holds
-- the changes from `parent` as JSON, `changes` their count.
CREATE TABLE revisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    parent INTEGER REFERENCES revisions (id),
    kind TEXT NOT NULL,
    restored_from INTEGER REFERENCES revisions (id),
    author_id INTEGER REFERENCES users (id) ON DELETE SET NULL,
    author TEXT,
    message TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    schema INTEGER NOT NULL,
    config TEXT NOT NULL,
    diff TEXT NOT NULL,
    changes INTEGER NOT NULL
) STRICT;
