-- The adoption port (features/adoption.md, Path C; D-106): where a pending device was found
-- (NULL gateway: its hello; else the gateway's node id and port), and whether root has a password
-- there, so nothing could be read of it. The gateway is a plain id, as the job is.
ALTER TABLE ztp_pending ADD COLUMN gateway INTEGER;
ALTER TABLE ztp_pending ADD COLUMN port TEXT;
ALTER TABLE ztp_pending ADD COLUMN locked INTEGER NOT NULL DEFAULT 0;
