-- 002_auth.sql
-- Drop hand-rolled sessions table (replaced by tower-sessions-sqlx-store, Pitfall 3)
-- tower-sessions-sqlx-store creates its own 'tower_sessions' table at startup via .migrate()
DROP TABLE IF EXISTS sessions;

-- Add provider identity columns to users (D-02)
-- SQLite requires one ALTER TABLE per column
ALTER TABLE users ADD COLUMN auth_provider TEXT NOT NULL DEFAULT 'password';
ALTER TABLE users ADD COLUMN external_id TEXT;  -- nullable: future OAuth sub claim
