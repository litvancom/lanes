-- Phase 1 Foundation: Full v1 schema (D-01)
-- Postgres-portable: TEXT UUIDs, INTEGER timestamps, no SQLite-isms (PLAT-01)
-- All 16 tables created here; later phases never modify this migration

CREATE TABLE IF NOT EXISTS users (
    id            TEXT PRIMARY KEY NOT NULL,  -- UUIDv7 hyphenated
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT,                        -- nullable: OAuth provider support later
    display_name  TEXT NOT NULL,
    avatar_color  TEXT NOT NULL DEFAULT '#78716c',
    created_at    INTEGER NOT NULL             -- epoch millis UTC (D-03)
);

CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY NOT NULL,  -- opaque token
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at  INTEGER NOT NULL            -- epoch millis UTC
);

CREATE TABLE IF NOT EXISTS boards (
    id            TEXT PRIMARY KEY NOT NULL,
    name          TEXT NOT NULL,
    key_prefix    TEXT NOT NULL,              -- Jira-style prefix e.g. "HOME" (D-02)
    next_card_num INTEGER NOT NULL DEFAULT 1, -- per-board card sequence counter (D-02)
    color         TEXT NOT NULL DEFAULT '#7c5cff',
    starred       INTEGER NOT NULL DEFAULT 0, -- BOOLEAN as 0/1 (Postgres-portable)
    archived      INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS board_members (
    board_id  TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    user_id   TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role      TEXT NOT NULL DEFAULT 'member', -- 'owner' | 'member'
    PRIMARY KEY (board_id, user_id)
);

CREATE TABLE IF NOT EXISTS lists (
    id        TEXT PRIMARY KEY NOT NULL,
    board_id  TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    name      TEXT NOT NULL,
    position  TEXT NOT NULL,               -- fractional index; ORDER BY position ASC (Pattern 5)
    archived  INTEGER NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_lists_board_position
    ON lists(board_id, position);

CREATE TABLE IF NOT EXISTS cards (
    id          TEXT PRIMARY KEY NOT NULL,
    list_id     TEXT NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    board_id    TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    card_num    INTEGER NOT NULL,           -- per-board sequence number for HOME-12 display (D-02)
    title       TEXT NOT NULL,
    description TEXT,
    cover       TEXT,                      -- CSS color/gradient or NULL
    position    TEXT NOT NULL,             -- fractional index; ORDER BY position ASC (Pattern 5)
    priority    TEXT,                      -- 'P1' | 'P2' | 'P3' | NULL
    due_at      INTEGER,                   -- epoch millis or NULL
    done        INTEGER NOT NULL DEFAULT 0,
    archived    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    UNIQUE(board_id, card_num)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_cards_list_position
    ON cards(list_id, position);

CREATE TABLE IF NOT EXISTS card_members (
    card_id  TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    user_id  TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (card_id, user_id)
);

CREATE TABLE IF NOT EXISTS labels (
    id        TEXT PRIMARY KEY NOT NULL,
    board_id  TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    name      TEXT NOT NULL,
    color     TEXT NOT NULL               -- oklch value string
);

CREATE TABLE IF NOT EXISTS card_labels (
    card_id   TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    label_id  TEXT NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
    PRIMARY KEY (card_id, label_id)
);

CREATE TABLE IF NOT EXISTS checklists (
    id        TEXT PRIMARY KEY NOT NULL,
    card_id   TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    title     TEXT NOT NULL,
    position  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS checklist_items (
    id            TEXT PRIMARY KEY NOT NULL,
    checklist_id  TEXT NOT NULL REFERENCES checklists(id) ON DELETE CASCADE,
    text          TEXT NOT NULL,
    done          INTEGER NOT NULL DEFAULT 0,
    position      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS comments (
    id          TEXT PRIMARY KEY NOT NULL,
    card_id     TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    author_id   TEXT NOT NULL REFERENCES users(id),
    body        TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    edited_at   INTEGER
);

CREATE TABLE IF NOT EXISTS attachments (
    id           TEXT PRIMARY KEY NOT NULL,
    card_id      TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    uploader_id  TEXT NOT NULL REFERENCES users(id),
    filename     TEXT NOT NULL,
    url          TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL,
    created_at   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS notifications (
    id          TEXT PRIMARY KEY NOT NULL,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    board_id    TEXT REFERENCES boards(id) ON DELETE CASCADE,
    card_id     TEXT REFERENCES cards(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,            -- 'mention' | 'assigned' | 'due_soon' | 'overdue'
    read        INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS invites (
    id          TEXT PRIMARY KEY NOT NULL,
    board_id    TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    inviter_id  TEXT NOT NULL REFERENCES users(id),
    email       TEXT NOT NULL,
    token       TEXT NOT NULL UNIQUE,
    expires_at  INTEGER NOT NULL,
    accepted    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS api_tokens (
    id           TEXT PRIMARY KEY NOT NULL,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER
);

CREATE TABLE IF NOT EXISTS watchers (
    card_id  TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    user_id  TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (card_id, user_id)
);
