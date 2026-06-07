-- 003_workspace.sql
-- Extend board_members for per-user starring and recent-view tracking (D-01, D-10)
--
-- Note: boards.starred is intentionally left in place.
-- SQLite cannot DROP COLUMN before version 3.35.0 (April 2021), and silently ignoring
-- the column is preferable to a destructive table recreate. All new queries read
-- board_members.starred (per-user, authoritative) and never write boards.starred.
-- The column can be cleaned up in a future migration once Postgres support is added.

-- Per-user starred flag (replaces board-level boards.starred per D-10)
-- NOT NULL with DEFAULT 0 so existing rows become unstarred immediately on migration
ALTER TABLE board_members ADD COLUMN starred INTEGER NOT NULL DEFAULT 0;

-- Nullable last_viewed_at: NULL = never opened; set to epoch millis on board open (D-01)
-- Top 3 boards by this value = "Recently viewed" section on workspace home
ALTER TABLE board_members ADD COLUMN last_viewed_at INTEGER;
