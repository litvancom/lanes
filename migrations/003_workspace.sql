-- 003_workspace.sql
-- Extend board_members for per-user starring and recent-view tracking (D-01, D-10)
-- SQLite ALTER TABLE supports ADD COLUMN only (no DROP COLUMN pre-3.35.0).
-- boards.starred is left in place and ignored — board_members.starred is authoritative.

-- Per-user starred flag (replaces board-level boards.starred for D-10).
-- NOT NULL with DEFAULT 0 so existing rows become unstarred immediately.
ALTER TABLE board_members ADD COLUMN starred INTEGER NOT NULL DEFAULT 0;

-- Nullable: NULL = never opened; set to epoch millis on board open (D-01).
ALTER TABLE board_members ADD COLUMN last_viewed_at INTEGER;
