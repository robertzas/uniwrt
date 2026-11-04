-- Applies (features/lifecycle.md, Render → diff → apply; D-069): an apply job names the
-- revision it applies and keeps how far each node got (a JSON array of model::job::JobNode).
ALTER TABLE jobs ADD COLUMN revision_id INTEGER;
ALTER TABLE jobs ADD COLUMN nodes TEXT NOT NULL DEFAULT '[]';

-- The revision each node holds, as its last apply (or a plan that found it unchanged) left it.
-- The job is a plain id, like the jobs table's node, so the record outlives job history.
CREATE TABLE node_applied (
    node_id INTEGER PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    revision_id INTEGER NOT NULL REFERENCES revisions (id),
    job_id INTEGER NOT NULL,
    applied_at INTEGER NOT NULL
) STRICT;
