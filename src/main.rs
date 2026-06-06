#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    // Load .env first (D-06)
    dotenvy::dotenv().ok();

    // Initialize tracing (D-06)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    start_server().await;
}

#[cfg(feature = "ssr")]
async fn start_server() {
    use lanes::server::{
        config::Config,
        db::{init_pools, run_migrations},
        state::{AppState, ReadPool, WritePool},
    };
    use lanes::app::App;
    use axum::Router;
    use leptos::config::get_configuration;
    use leptos_axum::{generate_route_list, LeptosRoutes};

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

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    let app_state = AppState {
        leptos_options: leptos_options.clone(),
        write_pool: WritePool(write_pool),
        read_pool: ReadPool(read_pool),
    };

    let app = Router::new()
        .leptos_routes(&app_state, routes, {
            let leptos_options = leptos_options.clone();
            move || {
                use leptos::prelude::*;
                view! {
                    <!DOCTYPE html>
                    <html lang="en">
                        <head>
                            <meta charset="utf-8"/>
                            <meta name="viewport" content="width=device-width, initial-scale=1"/>
                            <AutoReload options=leptos_options.clone() />
                            <HydrationScripts options=leptos_options.clone()/>
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
            view! {
                <!DOCTYPE html>
                <html lang="en">
                    <head>
                        <meta charset="utf-8"/>
                        <meta name="viewport" content="width=device-width, initial-scale=1"/>
                    </head>
                    <body>
                        <App/>
                    </body>
                </html>
            }
        }))
        .with_state(app_state);

    tracing::info!("Listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service()).await.unwrap();
}

#[cfg(not(feature = "ssr"))]
fn main() {}
