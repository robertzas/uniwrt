-- A floor's walls (features/wifi.md, Coverage & floor plans; M6.2c): a JSON array of segments
-- on its plan, each with a material, replaced whole by each edit.
ALTER TABLE floors ADD COLUMN walls TEXT NOT NULL DEFAULT '[]';
