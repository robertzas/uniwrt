-- A node reset to its defaults awaits its device's re-adoption with a pre-approval of its MAC
-- and board (D-131): the approval names the node whose record the device is adopted back into.
ALTER TABLE ztp_approvals ADD COLUMN node_id INTEGER REFERENCES nodes (id) ON DELETE CASCADE;
ALTER TABLE ztp_approvals ADD COLUMN board_name TEXT;
