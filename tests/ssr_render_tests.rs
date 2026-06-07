//! SSR render regression tests for components that use leptos-use hooks.
//!
//! These tests reproduce the SendWrapper cross-thread drop that previously caused
//! the server to abort on every authenticated home (WorkspacePage) request:
//!
//!   - `use_debounce_fn` in `WorkspaceTopbar` wraps its returned closure in
//!     `SendWrapper` (via `sendwrap_fn!`). When the reactive owner is dropped on a
//!     different tokio worker thread (the normal case with `rt-multi-thread`),
//!     the `StoredValue` cleanup fires on that thread, dropping the `SendWrapper`,
//!     which panics: "Dropped SendWrapper<T> variable from a thread different to
//!     the one it has been created with". A destructor panic on the poisoned RwLock
//!     in reactive_graph then aborts the process.
//!
//! Fix (Task 2): move `use_debounce_fn` and `use_event_listener` constructions
//! inside `Effect::new(...)`. Effects do not run during SSR, so no `SendWrapper`
//! state is allocated on the server.
//!
//! Run: cargo test --features ssr --test ssr_render_tests

#[cfg(feature = "ssr")]
mod ssr_render_tests {
    use leptos::prelude::*;
    use leptos_router::components::Router;
    use leptos_router::location::RequestUrl;
    use lanes::components::topbar::WorkspaceTopbar;
    use lanes::components::modal::Modal;

    /// Reproduces the pre-fix SendWrapper cross-thread drop panic for WorkspaceTopbar.
    ///
    /// WorkspaceTopbar uses `use_debounce_fn` which wraps the returned closure in
    /// `SendWrapper` (unconditional — not SSR-gated). When the reactive owner is
    /// dropped on a different worker thread, the StoredValue cleanup drops the
    /// SendWrapper on that thread, causing an abort.
    ///
    /// Before Task 2 fix: this test ABORTS the process.
    /// After Task 2 fix: owner drop completes without panic; HTML contains "lns-topbar".
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn topbar_renders_under_ssr() {
        let owner = Owner::new();
        let html = owner.with(|| {
            // WorkspaceTopbar calls use_navigate() at the top level, which requires
            // a RouterContext. Provide RequestUrl so the Router component can initialize
            // the context for SSR without needing a live server.
            provide_context(RequestUrl::new("/"));
            view! {
                <Router>
                    <WorkspaceTopbar
                        display_name="Alice".to_string()
                        on_new_board=Callback::new(|_| {})
                    />
                </Router>
            }
            .to_html()
        });

        // Drop the owner on a different worker thread — this is the cross-thread drop
        // that triggers the SendWrapper panic before the fix.
        tokio::task::spawn_blocking(move || {
            drop(owner);
        })
        .await
        .expect("owner drop on worker thread must not abort");

        assert!(
            html.contains("lns-topbar"),
            "rendered HTML must contain the topbar class; got: {html}"
        );
    }

    /// Verifies that Modal renders cleanly under SSR with owner drop on a worker thread.
    ///
    /// Modal uses `use_event_listener` which is already a no-op on SSR (feature-gated
    /// to `#[cfg(not(feature = "ssr"))]` in leptos-use). This test confirms the component
    /// remains clean after wrapping `use_event_listener` in `Effect::new`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn modal_renders_under_ssr() {
        let owner = Owner::new();
        let _html = owner.with(|| {
            let show = RwSignal::new(false);
            view! {
                <Modal show=show>
                    <p>"x"</p>
                </Modal>
            }
            .to_html()
        });

        // Drop on a different worker thread to verify no SendWrapper state leaks.
        tokio::task::spawn_blocking(move || {
            drop(owner);
        })
        .await
        .expect("owner drop on worker thread must not abort");
    }
}
