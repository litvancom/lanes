//! Settings page — API token management (API-03).
//!
//! Displays the user's personal API tokens, allows creating new tokens (raw shown once,
//! D-17 / D-18), and revoking existing tokens.
//!
//! Auth guard: redirects to /login when unauthenticated.
//!
//! Threat mitigations:
//! - T-07-15: token name rendered as Leptos text node (no inner_html; auto-escaped)
//! - T-07-16: raw token shown once in a read-only input; not stored after first render
//! - T-07-17: revoke is user-scoped on the server (no IDOR: DELETE WHERE id=? AND user_id=?)

use leptos::prelude::*;
use leptos_router::components::Redirect;
use crate::api::auth_api::get_current_user;
use crate::api::token_api::{create_api_token, list_api_tokens, revoke_api_token};
use crate::components::icon::Icon;
use crate::models::{ApiTokenMeta, CreatedToken};

// ---------------------------------------------------------------------------
// Auth guard shell
// ---------------------------------------------------------------------------

/// Settings page — auth guard, then renders SettingsContent.
#[component]
pub fn SettingsPage() -> impl IntoView {
    let current_user = Resource::new(|| (), |_| async { get_current_user().await });

    view! {
        <Suspense fallback=|| ()>
            {move || match current_user.get() {
                None => ().into_any(),
                Some(Ok(_user)) => view! { <SettingsContent/> }.into_any(),
                Some(Err(_)) => view! { <Redirect path="/login"/> }.into_any(),
            }}
        </Suspense>
    }
}

// ---------------------------------------------------------------------------
// Main settings content
// ---------------------------------------------------------------------------

#[component]
fn SettingsContent() -> impl IntoView {
    // Token list — refreshes after create/revoke
    let tokens_resource = Resource::new(|| (), |_| async { list_api_tokens().await });

    // Create token form state
    let token_name = RwSignal::new(String::new());
    let creating = RwSignal::new(false);
    let create_error = RwSignal::new(Option::<String>::None);
    // The raw token is shown once after successful creation; cleared on next create or navigate.
    let created_token = RwSignal::new(Option::<CreatedToken>::None);

    // Revoke action
    let revoke_action = Action::new(|token_id: &String| {
        let id = token_id.clone();
        async move { revoke_api_token(id).await }
    });

    // After a revoke, refresh the list
    let tokens_resource_rr = tokens_resource;
    Effect::new(move |_| {
        if revoke_action.value().get().is_some() {
            tokens_resource_rr.refetch();
        }
    });

    view! {
        <div class="lns-settings-layout">
            // --- Page header ---
            <header class="lns-settings-header">
                <h1 class="lns-settings-title">"Settings"</h1>
            </header>

            // --- API Tokens section ---
            <section class="lns-settings-section">
                <h2 class="lns-settings-section-title">"API Tokens"</h2>
                <p class="lns-settings-section-desc">
                    "Personal API tokens let you authenticate to the Lanes REST API. "
                    "Each token is shown once at creation and cannot be retrieved again."
                </p>

                // Create token form
                <div class="lns-settings-token-create">
                    <input
                        type="text"
                        class="lns-settings-token-name-input"
                        placeholder="Token name (e.g. CI pipeline)"
                        maxlength="80"
                        prop:value=move || token_name.get()
                        on:input=move |ev| {
                            token_name.set(event_target_value(&ev));
                            create_error.set(None);
                        }
                        on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                            if ev.key() == "Enter" {
                                let name = token_name.get();
                                if !name.trim().is_empty() && !creating.get() {
                                    creating.set(true);
                                    created_token.set(None);
                                    let tokens_res = tokens_resource;
                                    let creating_sig = creating;
                                    let created_sig = created_token;
                                    let error_sig = create_error;
                                    let name_sig = token_name;
                                    leptos::task::spawn_local(async move {
                                        match create_api_token(name).await {
                                            Ok(ct) => {
                                                created_sig.set(Some(ct));
                                                name_sig.set(String::new());
                                                tokens_res.refetch();
                                            }
                                            Err(e) => {
                                                error_sig.set(Some(e.to_string()));
                                            }
                                        }
                                        creating_sig.set(false);
                                    });
                                }
                            }
                        }
                    />
                    <button
                        type="button"
                        class="lns-btn lns-btn--primary lns-settings-token-create-btn"
                        disabled=move || creating.get() || token_name.get().trim().is_empty()
                        on:click=move |_| {
                            let name = token_name.get();
                            if !name.trim().is_empty() && !creating.get() {
                                creating.set(true);
                                created_token.set(None);
                                let tokens_res = tokens_resource;
                                let creating_sig = creating;
                                let created_sig = created_token;
                                let error_sig = create_error;
                                let name_sig = token_name;
                                leptos::task::spawn_local(async move {
                                    match create_api_token(name).await {
                                        Ok(ct) => {
                                            created_sig.set(Some(ct));
                                            name_sig.set(String::new());
                                            tokens_res.refetch();
                                        }
                                        Err(e) => {
                                            error_sig.set(Some(e.to_string()));
                                        }
                                    }
                                    creating_sig.set(false);
                                });
                            }
                        }
                    >
                        "Generate token"
                    </button>
                </div>

                // Validation error
                <Show when=move || create_error.get().is_some()>
                    <p class="lns-settings-error">
                        {move || create_error.get().unwrap_or_default()}
                    </p>
                </Show>

                // Show-once raw token reveal (D-17 / D-18)
                <Show when=move || created_token.get().is_some()>
                    {move || {
                        if let Some(ct) = created_token.get() {
                            view! {
                                <div class="lns-settings-token-reveal">
                                    <p class="lns-settings-token-reveal-warn">
                                        <strong>"Copy your token now."</strong>
                                        " It will not be shown again."
                                    </p>
                                    <div class="lns-settings-token-reveal-row">
                                        <code class="lns-settings-token-raw">{ct.raw_token.clone()}</code>
                                        <button
                                            type="button"
                                            class="lns-btn lns-btn--ghost lns-settings-token-copy-btn"
                                            title="Copy token"
                                            on:click={
                                                let raw = ct.raw_token.clone();
                                                move |_| {
                                                    #[cfg(feature = "hydrate")]
                                                    {
                                                        if let Some(window) = web_sys::window() {
                                                            let _ = window.navigator().clipboard().write_text(&raw);
                                                        }
                                                    }
                                                    #[cfg(not(feature = "hydrate"))]
                                                    let _ = &raw;
                                                }
                                            }
                                        >
                                            <Icon name="clipboard"/>
                                        </button>
                                    </div>
                                </div>
                            }.into_any()
                        } else {
                            ().into_any()
                        }
                    }}
                </Show>

                // Token list
                <Suspense fallback=|| view! { <p class="lns-settings-loading">"Loading tokens…"</p> }>
                    {move || {
                        let result = tokens_resource.get();
                        match result {
                            None => ().into_any(),
                            Some(Err(e)) => view! {
                                <p class="lns-settings-error">"Failed to load tokens: " {e.to_string()}</p>
                            }.into_any(),
                            Some(Ok(tokens)) => {
                                if tokens.is_empty() {
                                    view! {
                                        <p class="lns-settings-empty">"No API tokens yet."</p>
                                    }.into_any()
                                } else {
                                    view! {
                                        <ul class="lns-settings-token-list">
                                            <For
                                                each=move || tokens.clone()
                                                key=|t: &ApiTokenMeta| t.id.clone()
                                                children=move |token| {
                                                    let token_id = token.id.clone();
                                                    view! {
                                                        <li class="lns-settings-token-row">
                                                            <div class="lns-settings-token-info">
                                                                <span class="lns-settings-token-name">{token.name.clone()}</span>
                                                                <span class="lns-settings-token-meta">
                                                                    {format_created(token.created_at)}
                                                                </span>
                                                            </div>
                                                            <button
                                                                type="button"
                                                                class="lns-btn lns-btn--ghost lns-settings-token-revoke-btn"
                                                                title="Revoke token"
                                                                on:click=move |_| {
                                                                    revoke_action.dispatch(token_id.clone());
                                                                }
                                                            >
                                                                "Revoke"
                                                            </button>
                                                        </li>
                                                    }
                                                }
                                            />
                                        </ul>
                                    }.into_any()
                                }
                            }
                        }
                    }}
                </Suspense>
            </section>
        </div>
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format epoch millis as a short "Created YYYY-MM-DD" string.
/// Platform-safe: SSR uses std::time, WASM uses js_sys::Date.
fn format_created(epoch_ms: i64) -> String {
    // Simple epoch → date conversion (no chrono dep in shared code)
    // Days since Unix epoch
    let secs = epoch_ms / 1000;
    let days = secs / 86400;
    // Zeller / Jan 1 1970 was Thursday; compute year/month/day
    // Use a simple iterative approach — good enough for display
    let mut y = 1970i32;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let month_days: [i64; 12] = [
        31,
        if is_leap(y) { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut m = 1u32;
    for md in month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        m += 1;
    }
    let d = remaining + 1;
    format!("Created {y}-{m:02}-{d:02}")
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
