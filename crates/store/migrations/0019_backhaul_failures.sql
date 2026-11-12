-- Cables that failed the wired backhaul's probe (features/mesh.md, Wired backhaul; D-097): later
-- applies leave each a trunk while the same two ports face each other. One row per cable, its
-- ends in order; the job is a plain id, so the record outlives job history.
CREATE TABLE backhaul_failures (
    a_node INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    a_port TEXT NOT NULL,
    b_node INTEGER NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    b_port TEXT NOT NULL,
    job_id INTEGER NOT NULL,
    at INTEGER NOT NULL,
    why TEXT NOT NULL,
    PRIMARY KEY (a_node, a_port, b_node, b_port)
) STRICT;
