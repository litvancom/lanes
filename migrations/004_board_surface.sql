-- 004_board_surface.sql
-- Phase 4: Board Surface & Drag-Drop — additive schema additions only.
-- Mirrors the ALTER TABLE ADD COLUMN pattern from 003_workspace.sql.
-- SQLite cannot DROP COLUMN (before 3.35.0); all changes are purely additive.
--
-- D-13: is_done_list flag on lists — cards moved into this list have done=1 set
--       automatically (implemented in move_card_inner). NOT derived from list name.
--
-- D-11 / D-12: Denormalized seed-count columns on cards for thumbnail rendering.
--   checklist_done / checklist_total: set in seed; Phase 5 keeps them accurate via triggers/updates.
--   comment_count / attachment_count: same pattern.
--   cover: ALREADY EXISTS in 001_init.sql as `cover TEXT` — do NOT re-add here.

-- lists: is_done_list flag (D-13)
-- NOT NULL DEFAULT 0 so existing rows become non-done-lists on migration.
ALTER TABLE lists ADD COLUMN is_done_list INTEGER NOT NULL DEFAULT 0;

-- cards: denormalized thumbnail counts (D-11 / D-12)
-- All NOT NULL DEFAULT 0 so existing rows start at zero counts.
ALTER TABLE cards ADD COLUMN checklist_done   INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cards ADD COLUMN checklist_total  INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cards ADD COLUMN comment_count    INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cards ADD COLUMN attachment_count INTEGER NOT NULL DEFAULT 0;
