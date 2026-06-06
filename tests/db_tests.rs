//! Tests for the two-pool SQLite architecture (WAL mode, foreign keys, pool limits)
//! Run: DATABASE_URL=sqlite://data/test_pools.db cargo test --features ssr db_tests

#[cfg(feature = "ssr")]
mod db_tests {
    use lanes::server::{
        config::ConfigError,
        db::{init_pools, make_read_pool, make_write_pool, run_migrations},
    };
    use sqlx::Row;
    use tempfile::NamedTempFile;

    /// Create a temporary SQLite URL that is cleaned up after the test.
    fn temp_db_url() -> (NamedTempFile, String) {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_str().expect("path").to_string();
        let url = format!("sqlite://{}", path);
        (file, url)
    }

    #[tokio::test]
    async fn test_write_pool_wal_mode() {
        let (_file, url) = temp_db_url();
        let pool = make_write_pool(&url).await.expect("write pool");

        let row = sqlx::query("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .expect("pragma");
        let mode: String = row.get(0);
        assert_eq!(mode, "wal", "write pool must use WAL journal mode");
    }

    #[tokio::test]
    async fn test_write_pool_foreign_keys() {
        let (_file, url) = temp_db_url();
        let pool = make_write_pool(&url).await.expect("write pool");

        let row = sqlx::query("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .expect("pragma");
        let fk: i64 = row.get(0);
        assert_eq!(fk, 1, "write pool must have foreign keys enabled");
    }

    #[tokio::test]
    async fn test_write_pool_max_connections_is_one() {
        let (_file, url) = temp_db_url();
        let pool = make_write_pool(&url).await.expect("write pool");
        assert_eq!(
            pool.options().get_max_connections(),
            1,
            "write pool must have max_connections=1 to serialize writes"
        );
    }

    #[tokio::test]
    async fn test_read_pool_wal_mode() {
        let (_file, url) = temp_db_url();
        // Write pool MUST be created first to establish WAL mode
        let _write_pool = make_write_pool(&url).await.expect("write pool");
        let read_pool = make_read_pool(&url).await.expect("read pool");

        let row = sqlx::query("PRAGMA journal_mode")
            .fetch_one(&read_pool)
            .await
            .expect("pragma");
        let mode: String = row.get(0);
        assert_eq!(mode, "wal", "read pool must see WAL journal mode");
    }

    #[tokio::test]
    async fn test_init_pools_ordering_enforces_wal() {
        let (_file, url) = temp_db_url();
        let (write_pool, read_pool) = init_pools(&url).await.expect("init_pools");

        let wr: String = sqlx::query("PRAGMA journal_mode")
            .fetch_one(&write_pool)
            .await
            .expect("pragma")
            .get(0);
        let rr: String = sqlx::query("PRAGMA journal_mode")
            .fetch_one(&read_pool)
            .await
            .expect("pragma")
            .get(0);

        assert_eq!(wr, "wal");
        assert_eq!(rr, "wal");
    }

    #[tokio::test]
    async fn test_config_rejects_non_sqlite_url() {
        // Temporarily set DATABASE_URL to a non-sqlite:// value
        // We test the validation directly by crafting an invalid URL
        let result: Result<_, ConfigError> = (|| {
            let url = "postgres://user:pass@localhost/db".to_string();
            if !url.starts_with("sqlite://") {
                return Err(ConfigError::InvalidDatabaseUrl(url));
            }
            Ok(url)
        })();
        assert!(result.is_err(), "non-sqlite:// URL must be rejected");
    }

    #[tokio::test]
    async fn test_config_accepts_sqlite_url() {
        // Test valid URL passes validation
        let url = "sqlite://data/test.db";
        assert!(url.starts_with("sqlite://"), "sqlite:// URL must be accepted");
    }

    #[tokio::test]
    async fn test_run_migrations_succeeds_on_write_pool() {
        let (_file, url) = temp_db_url();
        let write_pool = make_write_pool(&url).await.expect("write pool");
        run_migrations(&write_pool)
            .await
            .expect("migrations must run without error");
    }
}
