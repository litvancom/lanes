use leptos::prelude::*;
use leptos::form::ActionForm;
use leptos_router::hooks::{use_navigate, use_query_map};
use crate::api::auth_api::Login;
use crate::components::logo::LogoMark;
use crate::components::checkbox::CustomCheckbox;

/// Login page: pixel-faithful recreation of design handoff screen 01 (AUTH-05).
///
/// Layout: two-panel row flex 100vh
///   Left — form panel (flex 1 1 580px, padding 40px 64px, bg --bg-elevated)
///   Right — brand panel (flex 1 1 580px, gradient bg, decorative)
///
/// Wired to ServerAction::<Login> via ActionForm.
/// Generic error "Invalid email or password." rendered via aria-live banner (D-18, T-02-08).
/// "or continue with" divider + OAuth buttons hidden with display:none when no providers configured (D-08).
///
/// # Return-to flow (D-12, T-02-23)
/// If a `?return=/invite/...` query param is present, successful login navigates client-side
/// to that path instead of the default workspace. SECURITY: only relative paths beginning with
/// a single `/` (not `//` and not containing a URL scheme) are honored; all others fall back to `/`.
#[component]
pub fn LoginPage() -> impl IntoView {
    let login_action = ServerAction::<Login>::new();
    let navigate = use_navigate();
    let query = use_query_map();

    // Extract and sanitize the ?return= query param (T-02-23 — open redirect prevention)
    // Only relative paths beginning with a single `/` (not `//` and no scheme) are honored.
    let return_path = move || {
        let q = query.read();
        let raw = q.get("return").unwrap_or_default();
        sanitize_return_path(&raw)
    };

    // On successful login: navigate to sanitized return-to path (or default workspace)
    Effect::new(move |_| {
        if let Some(Ok(_)) = login_action.value().get() {
            let dest = return_path();
            navigate(&dest, Default::default());
        }
    });

    // Derive the generic error message from the action result (D-18)
    // Both wrong password and unknown email produce the same message
    let login_error = move || {
        login_action.value().get().and_then(|r| match r {
            Err(e) => Some(e.to_string()),
            Ok(_) => None,
        })
    };

    view! {
        <div class="lns-screen">
            // Left: form panel
            <div class="lns-auth-form-panel">
                // Logo at top
                <div class="lns-auth-logo-row">
                    <LogoMark/>
                </div>

                // Vertically-centered form content
                <div class="lns-auth-form-content">
                    <h1 class="lns-auth-heading">"Welcome back"</h1>
                    <p class="lns-auth-subtext">"Sign in to keep moving things forward."</p>

                    <ActionForm action=login_action>
                        <div class="lns-auth-fields">
                            // Email field
                            <div class="lns-field">
                                <label class="lns-label" for="login-email">"Email"</label>
                                <input
                                    id="login-email"
                                    class="lns-input"
                                    type="email"
                                    name="email"
                                    autocomplete="email"
                                    placeholder="you@example.com"
                                    required
                                />
                            </div>

                            // Password field with inline "Forgot?" link
                            <div class="lns-field">
                                <div class="lns-field-label-row">
                                    <label class="lns-label" for="login-password">"Password"</label>
                                    <a class="lns-forgot-link" href="#" tabindex="-1">"Forgot?"</a>
                                </div>
                                <input
                                    id="login-password"
                                    class="lns-input"
                                    type="password"
                                    name="password"
                                    autocomplete="current-password"
                                    placeholder="••••••••"
                                    required
                                />
                            </div>

                            // Cosmetic "Keep me signed in" checkbox — default checked (D-07)
                            <CustomCheckbox
                                name="keep_signed_in"
                                label="Keep me signed in for 30 days"
                                checked=true
                            />

                            // Primary CTA button — dark background per design (not accent)
                            <button
                                class="lns-btn lns-btn--primary lns-btn--full"
                                type="submit"
                                disabled=move || login_action.pending().get()
                            >
                                "Sign in"
                            </button>

                            // "or continue with" divider + OAuth buttons
                            // D-08: hidden with display:none (not hidden attr — avoids hydration mismatch)
                            <div class="lns-oauth-section" style="display:none">
                                <div class="lns-oauth-divider">
                                    <div class="lns-oauth-divider-line"></div>
                                    <span class="lns-oauth-divider-text">"or continue with"</span>
                                    <div class="lns-oauth-divider-line"></div>
                                </div>
                                <div class="lns-oauth-buttons">
                                    <button class="lns-btn" type="button">
                                        // GitHub mark SVG
                                        <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
                                            <path d="M8 1a7 7 0 00-2.2 13.6c.35.06.48-.15.48-.34v-1.2c-1.95.42-2.36-.94-2.36-.94-.32-.81-.78-1.03-.78-1.03-.64-.43.05-.42.05-.42.7.05 1.07.72 1.07.72.62 1.07 1.64.76 2.04.58.06-.45.24-.76.44-.94-1.56-.18-3.2-.78-3.2-3.47 0-.77.27-1.4.72-1.89-.07-.18-.31-.9.07-1.88 0 0 .59-.19 1.93.72a6.7 6.7 0 013.52 0c1.34-.91 1.93-.72 1.93-.72.38.98.14 1.7.07 1.88.45.49.72 1.12.72 1.89 0 2.7-1.65 3.29-3.21 3.46.25.22.48.65.48 1.31v1.94c0 .19.13.41.49.34A7 7 0 008 1z"/>
                                        </svg>
                                        "GitHub"
                                    </button>
                                    <button class="lns-btn" type="button">
                                        // Google G SVG (brand fill)
                                        <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
                                            <path fill="#EA4335" d="M8 6.5v3h4.2A4.2 4.2 0 018 12.5a4.5 4.5 0 110-9 4.3 4.3 0 013 1.2l2.1-2.1A7.4 7.4 0 008 .5 7.5 7.5 0 108 15.5c4.3 0 7.2-3 7.2-7.3 0-.5 0-1-.1-1.7H8z"/>
                                        </svg>
                                        "Google"
                                    </button>
                                </div>
                            </div>

                            // Footer link (UI-SPEC Copywriting Contract)
                            <p class="lns-auth-switch-link">
                                "New to Lanes? "
                                <a class="lns-auth-link" href="/signup">"Create an account"</a>
                            </p>
                        </div>
                    </ActionForm>

                    // Generic login error (D-18) — aria-live for screen readers (UI-SPEC Accessibility)
                    <div aria-live="polite" class="lns-error-banner">
                        {move || login_error().map(|msg| view! {
                            <p class="lns-error-banner-text">{msg}</p>
                        })}
                    </div>
                </div>

                // Footer row (UI-SPEC Layout Contract)
                <footer class="lns-auth-footer">
                    <span class="lns-mono">"v0.4.2 · open source"</span>
                    <span>"Privacy · Terms · Help"</span>
                </footer>
            </div>

            // Right: brand panel — decorative, no interactive elements
            <div class="lns-auth-brand-panel">
                // Floating board preview card (rotate -1.5deg)
                <div class="lns-brand-card">
                    <div class="lns-brand-card-header">
                        <span class="lns-brand-card-dot"></span>
                        <span class="lns-brand-card-title">"Home & Life"</span>
                        <span class="lns-brand-card-meta">"4 lists · 23 cards"</span>
                    </div>
                    <div class="lns-brand-card-columns">
                        <div class="lns-brand-col">
                            <div class="lns-brand-col-header">"This week"</div>
                            <div class="lns-brand-mini-card">
                                <span class="lns-brand-label lns-brand-label--urgent"></span>
                                "Reply to landlord"
                            </div>
                            <div class="lns-brand-mini-card">
                                <span class="lns-brand-label lns-brand-label--errand"></span>
                                "Pick up dry cleaning"
                            </div>
                        </div>
                        <div class="lns-brand-col">
                            <div class="lns-brand-col-header">"In progress"</div>
                            <div class="lns-brand-mini-card">
                                <span class="lns-brand-label lns-brand-label--travel"></span>
                                "Plan Lisbon trip"
                            </div>
                            <div class="lns-brand-mini-card">
                                <span class="lns-brand-label lns-brand-label--someday"></span>
                                "Sort photos"
                            </div>
                        </div>
                        <div class="lns-brand-col">
                            <div class="lns-brand-col-header">"Done"</div>
                            <div class="lns-brand-mini-card lns-brand-mini-card--done">
                                <span class="lns-brand-label lns-brand-label--finance"></span>
                                "Auto-pay utilities"
                            </div>
                        </div>
                    </div>
                </div>

                // Floating "14 cards moved today" badge (rotate 3deg)
                <div class="lns-brand-badge">
                    <span class="lns-brand-badge-dot"></span>
                    "14 cards moved today"
                </div>

                // Floating "▸ DONE" dark label (rotate -4deg)
                <div class="lns-brand-done-label">
                    "▸ DONE"
                </div>

                // Bottom tagline (UI-SPEC Copywriting Contract)
                <div class="lns-brand-tagline-block">
                    <div class="lns-brand-tagline">"The kanban that respects your time."</div>
                    <div class="lns-brand-subtext">"Self-hosted, open source, and built to get out of your way."</div>
                </div>
            </div>
        </div>
    }
}

/// Sanitize a login `return` redirect target (T-02-23 — open redirect prevention).
///
/// Only permits relative paths that:
/// - Begin with exactly one `/`
/// - Do NOT begin with `//` (which browsers treat as protocol-relative and may redirect off-domain)
/// - Do NOT contain `:` before the first `/` (no URL schemes like `http:`)
///
/// Anything else defaults to `/` (workspace root).
fn sanitize_return_path(raw: &str) -> String {
    if raw.starts_with('/') && !raw.starts_with("//") && !raw.contains(':') {
        raw.to_string()
    } else {
        "/".to_string()
    }
}
