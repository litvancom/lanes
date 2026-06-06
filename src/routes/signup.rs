use leptos::prelude::*;
use leptos::form::ActionForm;
use crate::api::auth_api::Signup;

/// Signup page component: two-panel layout per UI-SPEC (D-16).
/// Uses .lns-* CSS class names so Plan 02's stylesheet binds without markup churn.
/// Full token-accurate CSS is deferred to Plan 02 — this task establishes functional markup.
#[component]
pub fn SignupPage() -> impl IntoView {
    let signup_action = ServerAction::<Signup>::new();

    // Derive field-specific error messages from the server function's error code (D-18, UI-SPEC)
    let email_error = move || {
        signup_action.value().get().and_then(|r| match r {
            Err(e) if e.to_string().contains("email_taken") => {
                Some("An account with this email already exists.")
            }
            _ => None,
        })
    };

    let password_error = move || {
        signup_action.value().get().and_then(|r| match r {
            Err(e) if e.to_string().contains("password_too_short") => {
                Some("Password must be at least 8 characters.")
            }
            _ => None,
        })
    };

    let name_error = move || {
        signup_action.value().get().and_then(|r| match r {
            Err(e) if e.to_string().contains("name_required") => {
                Some("Display name is required.")
            }
            _ => None,
        })
    };

    // Generic error for unexpected failures
    let generic_error = move || {
        signup_action.value().get().and_then(|r| match r {
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("email_taken")
                    || msg.contains("password_too_short")
                    || msg.contains("name_required")
                {
                    None // handled by field-specific errors
                } else {
                    Some("Something went wrong. Please try again.")
                }
            }
            _ => None,
        })
    };

    view! {
        <div class="lns-screen">
            // Form panel (left)
            <div class="lns-auth-panel-form">
                <div class="lns-auth-logo">
                    <div class="lns-logo-mark">
                        <span class="lns-logo-bar lns-logo-bar--short"></span>
                        <span class="lns-logo-bar lns-logo-bar--full"></span>
                        <span class="lns-logo-bar lns-logo-bar--accent"></span>
                    </div>
                    <span class="lns-logo-wordmark">"Lanes"</span>
                </div>

                <div class="lns-auth-form-content">
                    <h1 class="lns-auth-heading">"Create your account"</h1>
                    <p class="lns-auth-subtext">"Get started in seconds."</p>

                    <ActionForm action=signup_action>
                        // Display name field
                        <div class="lns-field">
                            <label class="lns-label" for="signup-name">"Display name"</label>
                            <input
                                id="signup-name"
                                class="lns-input"
                                class:lns-input--error=move || name_error().is_some()
                                type="text"
                                name="display_name"
                                autocomplete="name"
                                placeholder="Your name"
                                required
                            />
                            {move || name_error().map(|msg| view! {
                                <p class="lns-field-error">{msg}</p>
                            })}
                        </div>

                        // Email field
                        <div class="lns-field">
                            <label class="lns-label" for="signup-email">"Email"</label>
                            <input
                                id="signup-email"
                                class="lns-input"
                                class:lns-input--error=move || email_error().is_some()
                                type="email"
                                name="email"
                                autocomplete="email"
                                placeholder="you@example.com"
                                required
                            />
                            {move || email_error().map(|msg| view! {
                                <p class="lns-field-error">{msg}</p>
                            })}
                        </div>

                        // Password field
                        <div class="lns-field">
                            <label class="lns-label" for="signup-password">"Password"</label>
                            <input
                                id="signup-password"
                                class="lns-input"
                                class:lns-input--error=move || password_error().is_some()
                                type="password"
                                name="password"
                                autocomplete="new-password"
                                placeholder="At least 8 characters"
                                required
                            />
                            {move || password_error().map(|msg| view! {
                                <p class="lns-field-error">{msg}</p>
                            })}
                        </div>

                        <button
                            class="lns-btn lns-btn--primary lns-btn--full"
                            type="submit"
                            disabled=move || signup_action.pending().get()
                        >
                            "Create account"
                        </button>
                    </ActionForm>

                    // Generic error zone (below CTA)
                    {move || generic_error().map(|msg| view! {
                        <p class="lns-auth-error">{msg}</p>
                    })}

                    // Footer link
                    <p class="lns-auth-footer">
                        "Already have an account? "
                        <a class="lns-auth-link" href="/login">"Sign in"</a>
                    </p>
                </div>

                <footer class="lns-auth-footer-nav">
                    <span class="lns-version">"Lanes v0.1"</span>
                </footer>
            </div>

            // Brand panel (right, decorative — same as login per D-16)
            <div class="lns-auth-panel-brand">
                <p class="lns-brand-tagline">"The kanban that respects your time."</p>
            </div>
        </div>
    }
}
