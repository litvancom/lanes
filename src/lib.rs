// Increase recursion limit for complex type inference in WASM closures (Leptos drag-drop)
#![recursion_limit = "256"]

pub mod app;
pub mod models;
pub mod routes;
pub mod components;
pub mod api;
pub mod auth;
pub mod state;

#[cfg(feature = "ssr")]
pub mod server;

#[cfg(feature = "ssr")]
pub mod cli;

#[cfg(feature = "ssr")]
pub mod seed;

#[cfg(feature = "ssr")]
pub mod mailer;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use app::App;
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}
