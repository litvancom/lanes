/// CLI entry point types for the `lanes` binary.
///
/// Parsed in `main()` before the server starts. If `command` is
/// `Some(Commands::Seed)`, the seed path runs and exits; if `None`,
/// the Axum/Leptos server starts normally.
#[derive(clap::Parser, Debug)]
#[command(name = "lanes", about = "Lanes kanban server")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(clap::Subcommand, Debug)]
pub enum Commands {
    /// Seed demo data (fails if database is non-empty)
    Seed,
}
