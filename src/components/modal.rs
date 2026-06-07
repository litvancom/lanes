use leptos::prelude::*;
use leptos_use::{use_event_listener, use_window};

/// Reusable modal shell used across board creation (Phase 3) and card detail (Phase 5).
///
/// - Renders only when `show` is `true` (`<Show when>`).
/// - Backdrop click closes the modal (`show.set(false)`).
/// - Content click stops propagation so backdrop close is not triggered.
/// - Escape key closes the modal via `use_event_listener` inside `Effect::new`
///   (client-only — never runs during SSR to avoid SendWrapper cross-thread drops).
/// - `role="dialog"`, `aria-modal="true"`, `aria-labelledby` pointing at the
///   inner heading id (callers set `id="modal-heading"` on their `<h2>` or `<h3>`).
///
/// CSS: `.lns-modal-backdrop`, `.lns-modal-content` (defined in style/main.css).
/// Phase 5 card modal overrides content width to 760px via an inline style on the
/// wrapping element — this shell stays at 480px by default.
#[component]
pub fn Modal(
    /// Signal controlling whether the modal is open
    show: RwSignal<bool>,
    children: ChildrenFn,
) -> impl IntoView {
    // Escape key closes the modal.
    // Constructed inside Effect::new so it runs only on the client (Effects are
    // skipped during SSR). This prevents any SendWrapper state from being allocated
    // on the multi-threaded server runtime and subsequently dropped on a different
    // worker thread.
    Effect::new(move |_| {
        let _cleanup = use_event_listener(use_window(), leptos::ev::keydown, move |e: leptos::ev::KeyboardEvent| {
            if e.key() == "Escape" {
                show.set(false);
            }
        });
    });

    let on_backdrop_click = move |_| show.set(false);

    view! {
        <Show when=move || show.get()>
            // Backdrop — click anywhere outside content to close
            <div
                class="lns-modal-backdrop"
                on:click=on_backdrop_click
            />
            // Content — stop propagation so backdrop handler is not triggered
            <div
                class="lns-modal-content"
                role="dialog"
                aria-modal="true"
                aria-labelledby="modal-heading"
                on:click=|e| e.stop_propagation()
            >
                {children()}
            </div>
        </Show>
    }
}
