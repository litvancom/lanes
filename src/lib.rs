pub mod app;
pub mod models;
pub mod routes;
pub mod components;
pub mod api;

#[cfg(feature = "ssr")]
pub mod server;

#[cfg(feature = "ssr")]
pub mod cli;

#[cfg(feature = "ssr")]
pub mod seed;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use app::App;
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}
