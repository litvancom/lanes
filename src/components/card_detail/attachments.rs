//! Attachments section for the card-detail modal (Plan 05).
//!
//! Renders the attachment list and a sidebar-triggered file upload picker.
//! Upload goes to the plain Axum route POST /api/attachments/:board_id/:card_id (not a server fn).
//! UI-SPEC: section hidden when empty; generic file icon + filename + human-readable size + download link.
//! D-15: successful upload pushes the new Attachment into the modal-scoped signal AND increments
//!       attachment_count on the per-card RwSignal<Card> for thumbnail accuracy.

use leptos::prelude::*;
use crate::models::{Attachment, Card};
use crate::routes::board::BoardSignals;
use crate::components::icon::Icon;

/// Human-readable file size: "1.2 MB", "456 KB", "234 B"
fn fmt_size(bytes: i64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.0} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Attachments section: file list + upload button (triggered by sidebar "Attachment" button).
///
/// Props:
/// - `board_id`, `card_id`: for the upload URL and the per-card RwSignal lookup
/// - `attachments`: modal-scoped signal seeded from CardDetail.attachments
/// - `card_signal_key`: card_id to look up the per-card RwSignal<Card> in BoardSignals
/// - `file_input_id`: HTML id for the hidden file input — the sidebar button triggers it by id
#[component]
pub fn AttachmentsSection(
    board_id: String,
    card_id: String,
    attachments: RwSignal<Vec<Attachment>>,
    card_signal_key: String,
) -> impl IntoView {
    let board_id = StoredValue::new(board_id);
    let card_id = StoredValue::new(card_id);
    let card_signal_key = StoredValue::new(card_signal_key);

    let board_signals: Option<BoardSignals> = use_context::<BoardSignals>();

    // Upload error message signal (cleared on new upload attempt)
    let upload_error: RwSignal<Option<String>> = RwSignal::new(None);

    // The file input is placed inline; the sidebar "Attachment" button dispatches a click to it
    // via document.getElementById(id).click().
    // Derive the id from card_id (WR-07) so it is unique per card — a hardcoded id would
    // collide if two card-detail modals are ever mounted (e.g. during route transitions),
    // causing the sidebar trigger to target the wrong card's input.
    let file_input_id = format!("card-attachment-input-{}", card_id.get_value());

    view! {
        // Section hidden when no attachments AND no upload in progress (UI-SPEC line 357)
        <Show when=move || !attachments.get().is_empty()>
            <div class="lns-modal-section">
                <h4>
                    <Icon name="paperclip"/>
                    " Attachments"
                </h4>
                <div class="lns-attachment-list">
                    {move || attachments.get().into_iter().map(|att| {
                        let url = att.url.clone();
                        let filename = att.filename.clone();
                        let size_label = fmt_size(att.size_bytes);
                        view! {
                            <div class="lns-attachment-row">
                                <Icon name="file"/>
                                <div class="lns-attachment-info">
                                    <a
                                        class="lns-attachment-name"
                                        href=url.clone()
                                        download=filename.clone()
                                    >
                                        {filename.clone()}
                                    </a>
                                    <span class="lns-attachment-size">{size_label}</span>
                                </div>
                            </div>
                        }
                    }).collect_view()}
                </div>
            </div>
        </Show>

        // Hidden file input — triggered by the sidebar "Attachment" button
        // WASM-only: file upload requires browser APIs; rendered statically on SSR for hydration
        <input
            id=file_input_id
            type="file"
            style="display: none"
            on:change=move |ev| {
                #[cfg(target_arch = "wasm32")]
                {
                    use wasm_bindgen::JsCast;
                    use leptos::web_sys;

                    upload_error.set(None);

                    let input: web_sys::HtmlInputElement = ev.target()
                        .and_then(|t| t.dyn_into().ok())
                        .expect("file input element");

                    let files = input.files();
                    if files.is_none() { return; }
                    let files = files.unwrap();
                    if files.length() == 0 { return; }

                    let file = match files.get(0) {
                        Some(f) => f,
                        None => return,
                    };

                    let bid = board_id.get_value();
                    let cid = card_id.get_value();
                    let csk = card_signal_key.get_value();

                    // Build FormData with the file
                    let form_data = web_sys::FormData::new().expect("FormData");
                    form_data.append_with_blob("file", &file).expect("append file");

                    // D-05: extract own_client_id synchronously BEFORE spawn_local (sync read from untracked signal)
                    let cid_val = board_signals
                        .and_then(|bs| bs.own_client_id.get_untracked())
                        .unwrap_or_default();
                    let upload_url = format!("/api/attachments/{}/{}?client_id={}", bid, cid, cid_val);

                    // Spawn async fetch task
                    let error_sig = upload_error;
                    let attachments_sig = attachments;

                    leptos::task::spawn_local(async move {
                        let opts = web_sys::RequestInit::new();
                        opts.set_method("POST");
                        opts.set_body(&form_data);

                        let request = match web_sys::Request::new_with_str_and_init(&upload_url, &opts) {
                            Ok(r) => r,
                            Err(_) => {
                                error_sig.set(Some("Upload failed. Check file size and try again.".to_string()));
                                return;
                            }
                        };

                        let window = web_sys::window().expect("window");
                        let response_promise = window.fetch_with_request(&request);
                        let response_js = wasm_bindgen_futures::JsFuture::from(response_promise).await;

                        match response_js {
                            Ok(resp_val) => {
                                let response: web_sys::Response = resp_val.dyn_into().expect("Response");
                                if response.status() == 201 {
                                    // Parse the returned Attachment JSON
                                    let json_promise = response.json().expect("json()");
                                    if let Ok(json_val) = wasm_bindgen_futures::JsFuture::from(json_promise).await {
                                        // Deserialize via serde-wasm-bindgen is not available —
                                        // use JS text parsing via response.text() instead.
                                        // Re-fetch as text to deserialize with serde_json.
                                        // Actually we already consumed the body; use the parsed JS value directly.
                                        // Parse via js_sys::JSON::stringify + serde_json
                                        if let Ok(json_str) = js_sys::JSON::stringify(&json_val)
                                            .map(|s| s.as_string().unwrap_or_default())
                                        {
                                            if let Ok(att) = serde_json::from_str::<crate::models::Attachment>(&json_str) {
                                                // Push to modal-scoped signal
                                                attachments_sig.update(|v| v.push(att));
                                                // Write-through to per-card RwSignal<Card> (D-15)
                                                if let Some(bs) = board_signals {
                                                    bs.card_signals.with(|cs| {
                                                        if let Some(sig) = cs.get(&csk) {
                                                            sig.update(|c: &mut Card| c.attachment_count += 1);
                                                        }
                                                    });
                                                }
                                            }
                                        }
                                    }
                                } else if response.status() == 413 {
                                    error_sig.set(Some("Upload failed. Check file size and try again.".to_string()));
                                } else {
                                    error_sig.set(Some("Upload failed. Check file size and try again.".to_string()));
                                }
                            }
                            Err(_) => {
                                error_sig.set(Some("Upload failed. Check file size and try again.".to_string()));
                            }
                        }
                    });
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let _ = ev;
                }
            }
        />

        // Inline error message shown below the file input area (UI-SPEC copywriting)
        <Show when=move || upload_error.get().is_some()>
            <div class="lns-attachment-error">
                {move || upload_error.get().unwrap_or_default()}
            </div>
        </Show>
    }
}
