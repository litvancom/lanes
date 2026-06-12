-- 007_access_levels.sql — board access levels (viewer/commenter/editor + owner)
-- Rename the legacy read-write role to its new consistent name.
UPDATE board_members SET role = 'editor' WHERE role = 'member';

-- Invites carry the access level granted on acceptance (defaults preserve old behavior).
ALTER TABLE invites ADD COLUMN role TEXT NOT NULL DEFAULT 'editor';
