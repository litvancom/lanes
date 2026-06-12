-- 006_phase7.sql
-- Phase 7: Notification generators backbone (INBOX-01).
-- Additive migration only — no destructive changes.
--
-- Adds actor_id to notifications for inbox row anatomy (D-09):
--   kind values now include: 'mention' | 'assigned' | 'due_soon' | 'overdue' | 'watch_activity'
-- Adds dedup index for scheduler at-most-one-unread-per-kind-per-card constraint (D-05).

ALTER TABLE notifications ADD COLUMN actor_id TEXT REFERENCES users(id);

-- Index for scheduler dedup query (D-05 performance):
-- NOT EXISTS check in scan_due_notifications_once uses card_id + kind + read=0.
CREATE INDEX IF NOT EXISTS idx_notifications_card_kind_unread
    ON notifications(card_id, kind) WHERE read = 0;
