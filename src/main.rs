// Deeply nested Leptos view types push rustc's layout-query depth past the
// default limit of 128 when compiling the bin (error: "queries overflow the
// depth limit!"). Raise it per rustc's own suggestion.
#![recursion_limit = "256"]

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use clap::Parser;
    use lanes::cli::{Cli, Commands};

    // Load .env first (D-06)
    dotenvy::dotenv().ok();

    // Initialize tracing (D-06)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Seed) => {
            use lanes::server::{
                config::Config,
                db::{make_write_pool, run_migrations},
            };

            let config = Config::from_env().expect("Failed to load config");
            config.ensure_data_dir().expect("Failed to create data directory");

            let write_pool = make_write_pool(&config.database_url)
                .await
                .expect("Failed to create write pool");

            // run_migrations before run_seed (D-08 ordering)
            run_migrations(&write_pool)
                .await
                .expect("Failed to run database migrations");

            lanes::seed::run_seed(&write_pool)
                .await
                .expect("Seed failed");

            println!("Seed complete.");
        }
        Some(Commands::ResetPassword { email, new_password }) => {
            use lanes::server::{
                config::Config,
                db::{make_write_pool, run_migrations},
            };

            let config = Config::from_env().expect("Failed to load config");
            config.ensure_data_dir().expect("Failed to create data directory");

            let write_pool = make_write_pool(&config.database_url)
                .await
                .expect("Failed to create write pool");

            run_migrations(&write_pool)
                .await
                .expect("Failed to run database migrations");

            lanes::seed::reset_password(&write_pool, &email, &new_password)
                .await
                .expect("reset-password failed");

            println!("Password reset for {} complete.", email);
        }
        None => {
            start_server().await;
        }
    }
}

#[cfg(feature = "ssr")]
async fn start_server() {
    use std::sync::Arc;
    use lanes::server::{
        config::Config,
        db::{init_pools, run_migrations},
        state::{AppState, ReadPool, WritePool},
    };
    use lanes::app::App;
    use lanes::auth::backend::EmailPasswordBackend;
    use lanes::mailer::console::ConsoleMailer;
    use lanes::mailer::Mailer;
    use lanes::server::attachments::{upload_attachment_handler, download_attachment_handler};
    use lanes::server::board_rooms::BoardRoomRegistry;
    use lanes::server::user_notif_registry::UserNotifRegistry;
    use lanes::server::presence_registry::PresenceRegistry;
    use lanes::server::ws_handler::{ws_board_handler, ws_notifications_handler};
    use axum::Router;
    use leptos::config::get_configuration;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use tower_sessions::{Expiry, SessionManagerLayer};
    use tower_sessions::cookie::SameSite;
    use tower_sessions_sqlx_store::SqliteStore;
    use axum_login::AuthManagerLayerBuilder;
    use time::Duration;

    let config = Config::from_env().expect("Failed to load config");

    // Ensure data directory exists
    config.ensure_data_dir().expect("Failed to create data directory");

    // Two-pool SQLite wiring: write pool first (WAL), then read pool (D-05, T-01-04)
    let (write_pool, read_pool) = init_pools(&config.database_url)
        .await
        .expect("Failed to create database pools");

    // Run migrations on write pool only (Pattern 4)
    run_migrations(&write_pool)
        .await
        .expect("Failed to run database migrations");

    // --- Session + auth middleware wiring (RESEARCH Pattern 1) ---

    // tower-sessions SQLite store: creates 'tower_sessions' table at startup (Pitfall 3)
    // The hand-rolled `sessions` table was dropped in migration 002_auth.sql
    let session_store = SqliteStore::new(write_pool.clone());
    session_store
        .migrate()
        .await
        .expect("Failed to migrate session store");

    // 30-day sliding session (D-06); SameSite=Lax so invite GET links work (not Strict, anti-pattern note)
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(true)        // T-02-04: HttpOnly + Secure
        .with_http_only(true)
        .with_same_site(SameSite::Lax) // T-02-05: Lax allows GET from email links; mutations are POST
        .with_expiry(Expiry::OnInactivity(Duration::days(30))); // D-06: sliding 30-day

    let backend = EmailPasswordBackend::new(write_pool.clone(), read_pool.clone());
    let auth_layer = AuthManagerLayerBuilder::new(backend, session_layer).build();

    // Console mailer (D-13 floor); SMTP deferred to Phase 7
    let mailer: Arc<dyn Mailer> = Arc::new(ConsoleMailer);

    // Pluggable attachment storage: LocalFileSystem default, AmazonS3 when S3_BUCKET is set (DETAIL-08).
    // Derive attachments root from STORAGE_ROOT env var, falling back to a sibling dir next to the DB.
    let attachments_root = if let Ok(root) = std::env::var("STORAGE_ROOT") {
        std::path::PathBuf::from(root)
    } else {
        // db_file_path() is e.g. "data/lanes.db"; parent is "data/"; join "attachments" → "data/attachments"
        config
            .db_file_path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("data"))
            .join("attachments")
    };
    let storage = lanes::server::storage::init_storage(&attachments_root);

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    // Construct realtime registries (RT-01/RT-03/RT-04)
    let board_rooms = BoardRoomRegistry::new();
    let user_notifs = UserNotifRegistry::new();
    let presence = PresenceRegistry::new();

    let app_state = AppState {
        leptos_options: leptos_options.clone(),
        write_pool: WritePool(write_pool),
        read_pool: ReadPool(read_pool),
        mailer,
        storage,
        board_rooms,
        user_notifs,
        presence: presence.clone(),
    };

    // Presence sweep background task (Anti-Pattern §717: spawn once in start_server).
    // Every 10 seconds, reap viewers whose last_heartbeat is > 15s stale (D-13 / T-6-15).
    {
        use std::time::Instant;
        use std::time::Duration;
        let sweep_presence = presence;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                sweep_presence.sweep_once(Instant::now());
            }
        });
    }

    // Due-notification scheduler (INBOX-01 D-01/D-02/D-05): every 5 min, scan overdue/due_soon.
    {
        let sched_pool = app_state.write_pool.0.clone();
        let sched_notifs = app_state.user_notifs.clone();
        tokio::spawn(lanes::server::scheduler::run_due_notification_scheduler(sched_pool, sched_notifs));
    }

    // Read-notification cleanup (INBOX-01 D-10): hourly prune of read rows older than 30 days.
    {
        let cleanup_pool = app_state.write_pool.0.clone();
        tokio::spawn(lanes::server::scheduler::run_notif_cleanup(cleanup_pool));
    }

    let app = Router::new()
        // Attachment upload/download routes (plain Axum — not Leptos server fns, DETAIL-08).
        // Must be registered BEFORE .layer(auth_layer) so the session/auth layer wraps them.
        // Route-specific body limit for uploads (T-05-19, CR-03).
        // The handler enforces a 10 MB limit on the *file content*; this body limit
        // is set slightly higher (11 MB) to leave room for multipart boundaries and
        // headers, so a legitimate ~10 MB file is not rejected by Axum's whole-body
        // limit before the handler's friendlier 10 MB message can run.
        .route(
            "/api/attachments/{board_id}/{card_id}",
            axum::routing::post(upload_attachment_handler)
                .layer(axum::extract::DefaultBodyLimit::max(11 * 1024 * 1024)),
        )
        .route(
            "/api/attachments/{board_id}/{card_id}/{key}",
            axum::routing::get(download_attachment_handler),
        )
        // WebSocket route for realtime board sync (RT-01, T-6-01).
        // Axum 0.8 path syntax uses {param} (not :param).
        .route("/ws/board/{id}", axum::routing::get(ws_board_handler))
        // Per-user notification WebSocket (RT-04 / 06-06).
        // Auth via session cookie before upgrade; no board membership check required.
        .route("/ws/notifications", axum::routing::get(ws_notifications_handler))
        .leptos_routes(&app_state, routes, {
            let leptos_options = leptos_options.clone();
            move || {
                use leptos::prelude::*;
                use leptos_meta::MetaTags;
                view! {
                    <!DOCTYPE html>
                    <html lang="en">
                        <head>
                            <meta charset="utf-8"/>
                            <meta name="viewport" content="width=device-width, initial-scale=1"/>
                            <AutoReload options=leptos_options.clone() />
                            <HydrationScripts options=leptos_options.clone()/>
                            <MetaTags/>
                        </head>
                        <body>
                            <App/>
                        </body>
                    </html>
                }
            }
        })
        .fallback(leptos_axum::file_and_error_handler::<AppState, _>(|_| {
            use leptos::prelude::*;
            use leptos_meta::MetaTags;
            view! {
                <!DOCTYPE html>
                <html lang="en">
                    <head>
                        <meta charset="utf-8"/>
                        <meta name="viewport" content="width=device-width, initial-scale=1"/>
                        <MetaTags/>
                    </head>
                    <body>
                        <App/>
                    </body>
                </html>
            }
        }))
        .layer(auth_layer) // MUST be before .with_state() (RESEARCH Pattern 1)
        .with_state(app_state);

    tracing::info!("Listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service()).await.unwrap();
}

#[cfg(not(feature = "ssr"))]
fn main() {}
