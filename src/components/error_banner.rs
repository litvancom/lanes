use leptos::prelude::*;

/// ErrorBanner: aria-live="polite" region for announcing error messages.
///
/// Used for generic login errors ("Invalid email or password.") and
/// invite flow errors. Color: var(--danger).
/// Screen readers announce on change (UI-SPEC Accessibility Contract).
#[component]
pub fn ErrorBanner(
    /// The error message to display, or None for no error
    message: Signal<Option<String>>,
) -> impl IntoView {
    view! {
        <div aria-live="polite" class="lns-error-banner">
            {move || message.get().map(|msg| view! {
                <p class="lns-error-banner-text">{msg}</p>
            })}
        </div>
    }
}
