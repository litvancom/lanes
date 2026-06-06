//! Invite accept page: `/invite/:token`.
//!
//! Flow:
//! - Unauthenticated visitors are redirected to `/login?return=/invite/{token}` (D-12 return-to).
//! - Signed-in users with the matching email see a "Join board" CTA bound to accept_invite().
//! - On success the server redirects to `/board/{board_id}`.
//!
//! States handled: loading / wrong-email / expired / already-used / accepting / success (via redirect).
//!
//! # Security
//! - D-15: strict email binding enforced server-side in accept_invite()
//! - D-14: expiry + single-use enforced server-side in consume_invite()
//! - Error messages match UI-SPEC Copywriting Contract and Error Disclosure Rules
//! - Return-to redirects are relative paths only (SECURITY: T-02-23 open redirect prevented in LoginPage)

use leptos::prelude::*;
use leptos::form::ActionForm;
use leptos_router::params::Params;
use leptos_router::hooks::use_params;
use crate::api::invite_api::AcceptInvite;
use crate::components::logo::LogoMark;

/// Route params for `/invite/:token`.
#[derive(Params, PartialEq, Clone)]
struct InviteParams {
    token: Option<String>,
}

/// Invite accept page component.
///
/// Reads the `:token` route param via `use_params`, checks auth state via `get_current_user`,
/// and renders the accept form or bounces to `/login?return=/invite/{token}`.
#[component]
pub fn InviteAcceptPage() -> impl IntoView {
    let params = use_params::<InviteParams>();
    let token = move || {
        params.with(|p| {
            p.as_ref()
                .ok()
                .and_then(|p| p.token.clone())
                .unwrap_or_default()
        })
    };

    // Check auth state on page load (PATTERNS Resource + Suspense)
    let current_user = Resource::new(|| (), |_| async { crate::api::auth_api::get_current_user().await });

    // Accept action — bound to the "Join board" form below
    let accept_action = ServerAction::<AcceptInvite>::new();

    // Derive error copy from accept_action result
    let accept_error = move || {
        accept_action.value().get().and_then(|r| match r {
            Err(e) => Some(e.to_string()),
            Ok(_) => None,
        })
    };

    view! {
        // Single-panel variant: form panel only, no brand panel (UI-SPEC Layout Contract)
        <div class="lns-screen lns-screen--single">
            <div class="lns-auth-form-panel">
                <div class="lns-auth-logo-row">
                    <LogoMark/>
                </div>

                <div class="lns-auth-form-content">
                    <Suspense fallback=move || view! {
                        <p class="lns-auth-subtext">"Loading…"</p>
                    }>
                        {move || {
                            let tok = token();
                            current_user.get().map(|result| {
                                match result {
                                    // Unauthenticated — redirect to /login?return=/invite/{token} (D-12)
                                    Ok(None) | Err(_) => {
                                        let return_path = format!("/invite/{}", tok);
                                        let login_url = format!("/login?return={}", return_path);
                                        view! {
                                            <leptos_router::components::Redirect path=login_url/>
                                        }.into_any()
                                    }
                                    // Authenticated — show accept form
                                    Ok(Some(_user)) => view! {
                                        <div>
                                            <h1 class="lns-auth-heading">
                                                "You're invited to a board"
                                            </h1>
                                            <p class="lns-auth-subtext">
                                                "Accept this invitation to join the board and start collaborating."
                                            </p>

                                            // Accept form — hidden token field + "Join board" CTA
                                            // (ActionForm + ServerAction::<AcceptInvite> per PATTERNS)
                                            <ActionForm action=accept_action>
                                                <input type="hidden" name="token" value=tok.clone()/>
                                                <div class="lns-auth-fields">
                                                    <button
                                                        class="lns-btn lns-btn--primary lns-btn--full"
                                                        type="submit"
                                                        disabled=move || accept_action.pending().get()
                                                    >
                                                        "Join board"
                                                    </button>
                                                </div>
                                            </ActionForm>

                                            // Error banner — wrong-email / expired / already-used copy
                                            // (rendered for screen readers via aria-live)
                                            <div aria-live="polite" class="lns-error-banner">
                                                {move || accept_error().map(|msg| view! {
                                                    <p class="lns-error-banner-text">{msg}</p>
                                                })}
                                            </div>
                                        </div>
                                    }.into_any(),
                                }
                            })
                        }}
                    </Suspense>
                </div>

                <footer class="lns-auth-footer">
                    <span class="lns-mono">"v0.4.2 · open source"</span>
                    <span>"Privacy · Terms · Help"</span>
                </footer>
            </div>
        </div>
    }
}
