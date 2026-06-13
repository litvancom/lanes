//! Hydration-safety flag.
//!
//! SSR and the browser run as two separate binaries (native server + `wasm32`).
//! Anything derived from the wall clock — `now_ms()`, relative timestamps, "today"
//! highlighting — is computed differently on each side, so if it's emitted during
//! SSR it won't match what the client builds on its first hydrate render. tachys
//! then hits `unreachable` and the page fails to hydrate.
//!
//! The fix: gate clock-derived (or otherwise nondeterministic) rendering on
//! [`use_hydrated`]. It is `false` during SSR and the initial hydrate pass — so
//! both sides emit an identical, neutral baseline — then flips `true` after the
//! client mounts, letting the real value fill in reactively, post-hydration.

use leptos::prelude::*;

/// App-wide hydration flag (see module docs). Provided once by `App`.
#[derive(Clone, Copy)]
pub struct Hydrated(pub RwSignal<bool>);

/// Provide the hydration flag. Call once, at the top of `App`.
pub fn provide_hydrated() {
    let flag = RwSignal::new(false);
    // Effects never run during SSR and run only after the first client render,
    // so this flips to `true` exactly once hydration has completed.
    Effect::new(move |_| flag.set(true));
    provide_context(Hydrated(flag));
}

/// Read the hydration flag. Falls back to an always-`true` signal when no
/// provider is present (e.g. components rendered in isolation by tests), which
/// is safe because those paths don't hydrate.
pub fn use_hydrated() -> RwSignal<bool> {
    use_context::<Hydrated>()
        .map(|h| h.0)
        .unwrap_or_else(|| RwSignal::new(true))
}
