use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::SqlitePool;
use std::str::FromStr;
use std::time::Duration;

/// Create the write pool with WAL mode and max_connections=1 to serialize all writes.
/// MUST be called before make_read_pool to ensure WAL mode is set first (T-01-04, Pitfall 3).
pub async fn make_write_pool(url: &str) -> sqlx::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_millis(5000))
        .foreign_keys(true);

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
}

/// Create the read pool. Must be called AFTER make_write_pool so WAL mode is already active.
pub async fn make_read_pool(url: &str) -> sqlx::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(url)?
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_millis(5000))
        .foreign_keys(true)
        .read_only(true);

    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
}

/// Initialize both pools in the correct order: write pool first, then read pool.
/// This ordering is mandatory to ensure WAL mode is applied before read connections are opened.
pub async fn init_pools(url: &str) -> sqlx::Result<(SqlitePool, SqlitePool)> {
    let write_pool = make_write_pool(url).await?;
    let read_pool = make_read_pool(url).await?;
    Ok((write_pool, read_pool))
}

/// Run all pending migrations against the write pool only.
/// Migrations are idempotent — safe to call on every startup.
pub async fn run_migrations(write_pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(write_pool).await
}
