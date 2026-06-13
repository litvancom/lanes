# Lanes

A fast, beautiful, self-hostable **kanban board** — built full-stack in Rust with [Leptos](https://leptos.dev). Multiple users share boards and see changes **live**, themed around personal life admin (errands, trips, finances, reading lists).

> The kanban that respects your time. Self-hosted, open source, and built to get out of your way.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)

## Features

- **Live multi-user sync** — drag a card, rename a list, drop a comment; everyone on the board sees it instantly (WebSocket-backed reactive signals).
- **Board sharing with roles** — invite by email as **viewer**, **commenter**, **editor**, or **owner**; owners manage members.
- **Cards that do the job** — drag-and-drop ordering, inline add/edit, labels, due dates, priorities, covers, comments, attachments, checklists, watchers.
- **Workspace tools** — recent/starred boards, full-text search, an **inbox** for notifications, and a **calendar** view of due dates.
- **Pluggable backends** — SQLite by default (Postgres-portable schema), local-disk attachments by default (S3-compatible when configured), email/password auth with a swappable provider abstraction.
- **Self-host friendly** — ships as a single Docker image; data persists on one volume.

## Tech stack

| Layer | Choice |
|-------|--------|
| UI | Leptos 0.8 (SSR + WASM hydration) |
| Server | Axum 0.8 + Tokio |
| Database | SQLx 0.8 + SQLite (WAL, single-writer + read pool) |
| Auth | tower-sessions + axum-login, Argon2id (`password-auth`) |
| Realtime | `leptos_ws` over Axum WebSockets |
| Storage | `object_store` (local FS / S3) |
| Drag & drop | Native HTML5 DnD via Leptos event handlers |

## Quick start (Docker)

```bash
# Build and run — empty DB; sign up to create your first account
docker compose up -d --build

# …or load demo fixtures on first boot (only seeds an empty DB)
SEED_DEMO=true docker compose up -d --build
```

Then open <http://localhost:3000>.

Data (the SQLite DB + uploaded attachments) lives on the `lanes_data` volume and survives container recreation.

## Configuration

Copy `.env.example` to `.env` and adjust. Common settings:

| Variable | Default | Purpose |
|----------|---------|---------|
| `DATABASE_URL` | `sqlite:///data/lanes.db` | SQLite database path |
| `LEPTOS_SITE_ADDR` | `0.0.0.0:3000` | Bind address |
| `STORAGE_ROOT` | `/data/attachments` | Local attachment directory |
| `COOKIE_SECURE` | `true` | Set `false` for plain-HTTP (LAN) deployments |
| `RUST_LOG` | `info` | Log level |

Set the S3 (`AWS_*`) and SMTP variables in `.env.example` to switch attachment storage to an S3-compatible bucket and enable real email delivery. Without SMTP, invite and password-reset links are printed to the logs.

> **Note:** `COOKIE_SECURE` defaults to secure (HTTPS-only) cookies. Behind plain HTTP (e.g. a LAN homelab) you must set `COOKIE_SECURE=false`, or logins won't stick.

## Development

Lanes uses [`cargo-leptos`](https://github.com/leptos-rs/cargo-leptos) to dual-compile the server binary and the WASM bundle.

```bash
# One-time tooling
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos --locked
cargo install sqlx-cli --no-default-features --features sqlite

# Hot-reloading dev server (server + WASM + SCSS), serves http://127.0.0.1:3000
cargo leptos watch
```

The repo commits an offline SQLx query cache (`.sqlx/`), so builds work with `SQLX_OFFLINE=true` and need no live database. After changing any `sqlx::query!` macro or a migration, regenerate it:

```bash
cargo sqlx prepare           # requires DATABASE_URL pointing at a migrated DB
```

### Tests

```bash
cargo test --no-default-features --features ssr
```

## Deployment notes

- **Keep a single replica.** SQLite is single-writer; run exactly one instance against one data volume. Don't scale horizontally on a shared SQLite file.
- Mount a persistent volume at `/data`.
- Terminate TLS at a reverse proxy (or set `COOKIE_SECURE=false` for HTTP-only).

## Contributing

Issues and pull requests are welcome — see [CONTRIBUTING.md](./CONTRIBUTING.md). Security reports: see [SECURITY.md](./SECURITY.md).

## License

[MIT](./LICENSE) © Ivan Litovchenko
