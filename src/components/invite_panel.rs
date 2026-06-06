use leptos::prelude::*;
use leptos::form::ActionForm;
use crate::api::invite_api::InviteMember;
use crate::components::form_field::FormField;
use crate::components::error_banner::ErrorBanner;

/// InvitePanel: owner-facing panel to invite users to a board by email.
///
/// States (UI-SPEC Component Inventory):
/// - idle: shows email input + "Send invite" CTA
/// - sending: action pending (ActionForm handles this via action.pending())
/// - link-displayed: action succeeded, copyable invite link shown (D-13)
/// - error: action returned Err, ErrorBanner shows the message
///
/// The invite link is always shown on success regardless of email delivery (D-13).
/// Copy: "Invite to board", "Email address", "Send invite", "Share this link",
///       "This link works even if the email didn't arrive.", "Copy link", "Copied!"
///       (UI-SPEC Copywriting Contract)
#[component]
pub fn InvitePanel(
    /// The board ID to invite into (hidden form field)
    board_id: String,
) -> impl IntoView {
    let invite_action = ServerAction::<InviteMember>::new();

    // Tracks the clipboard copy state ("Copy link" → "Copied!")
    let copied = RwSignal::new(false);

    // When the action succeeds with a new invite URL, reset the copied state
    Effect::new(move |_| {
        if let Some(Ok(_)) = invite_action.value().get() {
            copied.set(false);
        }
    });

    // Extract error message from action result for ErrorBanner
    let error_message = Signal::derive(move || {
        invite_action.value().get().and_then(|result| match result {
            Err(e) => Some(e.to_string()),
            Ok(_) => None,
        })
    });

    view! {
        <div class="lns-invite-panel">
            <h2 class="lns-invite-panel-heading">"Invite to board"</h2>

            // Error banner — shown when action returns Err (e.g. owner-only rejection)
            <ErrorBanner message=error_message />

            // Always show the form so the owner can re-invite
            <ActionForm action=invite_action>
                <input type="hidden" name="board_id" value=board_id />
                <FormField
                    id="invite-email"
                    name="email"
                    label="Email address"
                    input_type="email"
                    autocomplete="email"
                    placeholder="colleague@example.com"
                />
                <button
                    type="submit"
                    class="lns-btn lns-btn--primary lns-btn--full"
                    disabled=move || invite_action.pending().get()
                >
                    {move || if invite_action.pending().get() {
                        "Sending..."
                    } else {
                        "Send invite"
                    }}
                </button>
            </ActionForm>

            // Link-displayed state: always shown when action returns Ok (D-13)
            {move || invite_action.value().get().and_then(|result| result.ok()).map(|invite_url| {
                let url_for_display = invite_url.clone();

                view! {
                    <div class="lns-invite-link-block">
                        <p class="lns-invite-link-label">"Share this link"</p>
                        <div class="lns-invite-link-row">
                            <input
                                type="text"
                                class="lns-input lns-invite-link-input"
                                readonly=true
                                value=url_for_display.clone()
                            />
                            <button
                                type="button"
                                class="lns-btn lns-invite-copy-btn"
                                on:click={
                                    let url = url_for_display.clone();
                                    move |_| {
                                        // Clipboard copy: the link input is readonly and selectable.
                                        // WASM-side copy via wasm_bindgen eval (no web-sys feature flags needed).
                                        #[cfg(target_arch = "wasm32")]
                                        {
                                            use wasm_bindgen::prelude::*;
                                            #[wasm_bindgen]
                                            extern "C" {
                                                fn eval(s: &str) -> JsValue;
                                            }
                                            let script = format!(
                                                "navigator.clipboard && navigator.clipboard.writeText({:?})",
                                                url
                                            );
                                            eval(&script);
                                        }
                                        let _ = &url; // suppress unused-variable in SSR build
                                        copied.set(true);
                                    }
                                }
                            >
                                {move || if copied.get() { "Copied!" } else { "Copy link" }}
                            </button>
                        </div>
                        <p class="lns-invite-link-helper">
                            "This link works even if the email didn't arrive."
                        </p>
                    </div>
                }
            })}
        </div>
    }
}
