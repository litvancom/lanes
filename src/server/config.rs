use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub site_addr: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("DATABASE_URL must start with 'sqlite://', got: {0}")]
    InvalidDatabaseUrl(String),
    #[error("Failed to create database directory: {0}")]
    DirectoryCreation(#[from] std::io::Error),
}

impl Config {
    /// Load config from environment variables (D-06)
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://data/lanes.db".to_string());

        // T-01-01: Validate database_url starts with sqlite://
        if !database_url.starts_with("sqlite://") {
            return Err(ConfigError::InvalidDatabaseUrl(database_url));
        }

        let site_addr = std::env::var("LEPTOS_SITE_ADDR")
            .or_else(|_| std::env::var("LANES_SITE_ADDR"))
            .unwrap_or_else(|_| "127.0.0.1:3000".to_string());

        Ok(Config {
            database_url,
            site_addr,
        })
    }

    /// Extract the filesystem path from the sqlite:// URL and ensure the parent directory exists.
    pub fn ensure_data_dir(&self) -> Result<(), ConfigError> {
        let path = self.db_file_path();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        Ok(())
    }

    /// Extract the filesystem path from "sqlite://path/to/file.db"
    pub fn db_file_path(&self) -> PathBuf {
        // sqlite://data/lanes.db -> data/lanes.db
        let path_str = self.database_url.trim_start_matches("sqlite://");
        PathBuf::from(path_str)
    }
}
