-- The feeds each node's package manager reads (features/lifecycle.md, Offline packages; D-147):
-- a JSON array of model::inventory::NodeFeed, read with the node's packages.
ALTER TABLE node_inventory ADD COLUMN feeds TEXT NOT NULL DEFAULT '[]';
