-- 005_card_detail.sql
-- Phase 5: Card Detail — additive migration only (no destructive changes).
--
-- Note: `watchers` table (card_id, user_id PK) already exists in 001_init.sql
--       (lines 153-157) — do NOT recreate it here.
-- Note: `cards.description` already exists in 001_init.sql (line 56) — do NOT re-add.
-- Note: All other detail tables (comments, attachments, checklists, checklist_items,
--       card_members, labels, card_labels, notifications) already exist in 001_init.sql.
--
-- This migration adds only the card_events activity table (D-08, A4).

CREATE TABLE IF NOT EXISTS card_events (
    id         TEXT PRIMARY KEY NOT NULL,
    card_id    TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    actor_id   TEXT REFERENCES users(id),
    kind       TEXT NOT NULL,
    -- kind values: 'created'|'moved'|'archived'|'member_added'|'member_removed'
    payload    TEXT,           -- JSON: e.g. {"from_list":"...","to_list":"..."}
    created_at INTEGER NOT NULL
);
