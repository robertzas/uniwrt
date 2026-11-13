-- Floor plans (features/wifi.md, Coverage & floor plans; D-117): each floor with its image and
-- scale, and the nodes placed on it (one floor per node at most). Positions are in the image's
-- pixels. Deleting a floor or a node takes its placements with it.
CREATE TABLE floors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    level INTEGER NOT NULL UNIQUE,
    height_m REAL NOT NULL,
    scale TEXT,
    image_kind TEXT,
    image_width INTEGER,
    image_height INTEGER,
    image_version INTEGER NOT NULL DEFAULT 0,
    image BLOB,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE floor_nodes (
    node_id INTEGER PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    floor_id INTEGER NOT NULL REFERENCES floors (id) ON DELETE CASCADE,
    x REAL NOT NULL,
    y REAL NOT NULL
) STRICT;

CREATE INDEX floor_nodes_floor ON floor_nodes (floor_id);
