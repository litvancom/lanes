use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use crate::api::workspace_api::{AddBoard, AddBoardFromTemplate, BOARD_COLOR_SWATCHES};
use crate::components::modal::Modal;

/// Templates available for board creation (D-07).
/// Each tuple is (slug, display_name, dot_color).
const TEMPLATES: &[(&str, &str, &str)] = &[
    ("personal_todos", "Personal todos", "#7c5cff"),
    ("weekly_review",  "Weekly review",  "#10b981"),
    ("trip_planning",  "Trip planning",  "#0ea5e9"),
];

/// Board creation modal (D-05).
///
/// Collects: name (auto-focused), color swatch (one of 5), optional template.
/// On submit:
/// - Blank board  → dispatches `AddBoard { name, color }`
/// - Template     → dispatches `AddBoardFromTemplate { name, color, template }`
/// On success → closes modal + navigates to `/board/:new_id` (D-05).
/// Error → inline `aria-live="polite"` error region.
///
/// Threat mitigations:
/// - Color is always from BOARD_COLOR_SWATCHES (T-03-13); never free text.
/// - Template slug is always from TEMPLATES const (T-03-15).
/// - Leptos view! escapes all text node content (T-03-14).
/// - Server fns re-validate name/color/template independently.
#[component]
pub fn CreateBoardModal(
    /// Signal controlling whether the modal is open
    show: RwSignal<bool>,
) -> impl IntoView {
    // --- Local form state ---
    let name = RwSignal::new(String::new());
    let selected_color = RwSignal::new(BOARD_COLOR_SWATCHES[0].to_string());
    let selected_template = RwSignal::<Option<String>>::new(None);
    // Client-side validation error (separate from server error)
    let local_error = RwSignal::<Option<String>>::new(None);

    // --- Server actions ---
    let add_blank   = ServerAction::<AddBoard>::new();
    let add_tmpl    = ServerAction::<AddBoardFromTemplate>::new();

    // --- Auto-focus the name input when the modal opens ---
    let name_ref = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if show.get() {
            if let Some(input) = name_ref.get() {
                let _ = input.focus();
            }
        }
    });

    // --- Reset local state when modal opens ---
    Effect::new(move |_| {
        if show.get() {
            name.set(String::new());
            selected_color.set(BOARD_COLOR_SWATCHES[0].to_string());
            selected_template.set(None);
            local_error.set(None);
        }
    });

    // --- Navigate on success ---
    let navigate = use_navigate();
    Effect::new(move |_| {
        let blank_val = add_blank.value().get();
        let tmpl_val  = add_tmpl.value().get();

        if let Some(Ok(board)) = blank_val {
            show.set(false);
            navigate(&format!("/board/{}", board.id), Default::default());
        } else if let Some(Ok(board)) = tmpl_val {
            show.set(false);
            navigate(&format!("/board/{}", board.id), Default::default());
        }
    });

    // --- Submit handler ---
    let on_submit = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        let board_name = name.get();
        let trimmed = board_name.trim().to_string();
        if trimmed.is_empty() {
            local_error.set(Some("Board name cannot be empty".into()));
            return;
        }
        if trimmed.chars().count() > 120 {
            local_error.set(Some("Board name is too long (120 characters max)".into()));
            return;
        }
        local_error.set(None);
        let color = selected_color.get();

        match selected_template.get() {
            Some(tmpl) => {
                add_tmpl.dispatch(AddBoardFromTemplate {
                    name: trimmed,
                    color,
                    template: tmpl,
                });
            }
            None => {
                add_blank.dispatch(AddBoard {
                    name: trimmed,
                    color,
                });
            }
        }
    };

    // --- Derived server error ---
    let server_error = move || {
        let blank_err = add_blank.value().get().and_then(|r| r.err());
        let tmpl_err  = add_tmpl.value().get().and_then(|r| r.err());
        blank_err.or(tmpl_err).map(|e| e.to_string())
    };

    let is_pending = move || add_blank.pending().get() || add_tmpl.pending().get();

    view! {
        <Modal show=show>
            // Heading — referenced by aria-labelledby="modal-heading" on .lns-modal-content
            <h2 id="modal-heading" class="lns-modal-title">"Create board"</h2>

            // ── Name field ────────────────────────────────────────────
            <div class="lns-modal-field">
                <label class="lns-label" for="board-name-input">"Board name"</label>
                <input
                    id="board-name-input"
                    type="text"
                    class="lns-input"
                    placeholder="Board name"
                    node_ref=name_ref
                    prop:value=move || name.get()
                    on:input=move |ev| name.set(event_target_value(&ev))
                />
            </div>

            // ── Color swatches ────────────────────────────────────────
            <div class="lns-modal-field">
                <span class="lns-label">"Color"</span>
                <div class="lns-color-swatches" role="radiogroup" aria-label="Board color">
                    {BOARD_COLOR_SWATCHES.iter().map(|&color| {
                        let color_owned = color.to_string();
                        let c1 = color_owned.clone();
                        let c2 = color_owned.clone();
                        let c3 = color_owned.clone();
                        let c4 = color_owned.clone();
                        view! {
                            <button
                                type="button"
                                role="radio"
                                aria-checked=move || selected_color.get() == c1
                                aria-label=c2.clone()
                                class=move || {
                                    if selected_color.get() == c3 {
                                        "lns-color-swatch lns-color-swatch--selected"
                                    } else {
                                        "lns-color-swatch"
                                    }
                                }
                                style=move || format!("background-color: {c4};")
                                on:click=move |_| selected_color.set(color_owned.clone())
                            />
                        }
                    }).collect_view()}
                </div>
            </div>

            // ── Template tiles ────────────────────────────────────────
            <div class="lns-modal-field">
                <span class="lns-section-label">"Start from a template"</span>
                <div class="lns-template-tiles" role="radiogroup" aria-label="Board template">
                    {TEMPLATES.iter().map(|&(slug, label, dot_color)| {
                        let slug_owned = slug.to_string();
                        let s1 = slug_owned.clone();
                        let s2 = slug_owned.clone();
                        let s3 = slug_owned.clone();
                        view! {
                            <button
                                type="button"
                                role="radio"
                                aria-checked=move || selected_template.get().as_deref() == Some(s1.as_str())
                                class=move || {
                                    if selected_template.get().as_deref() == Some(s2.as_str()) {
                                        "lns-template-tile lns-template-tile--selected"
                                    } else {
                                        "lns-template-tile"
                                    }
                                }
                                on:click=move |_| {
                                    let is_selected = selected_template.get().as_deref() == Some(s3.as_str());
                                    if is_selected {
                                        selected_template.set(None);
                                    } else {
                                        selected_template.set(Some(slug_owned.clone()));
                                    }
                                }
                            >
                                <span
                                    class="lns-template-tile-dot"
                                    style=format!("background-color: {dot_color};")
                                />
                                <p class="lns-template-tile-name">{label}</p>
                                <div class="lns-template-tile-bars">
                                    <div class="lns-template-tile-bar"/>
                                    <div class="lns-template-tile-bar"/>
                                    <div class="lns-template-tile-bar"/>
                                </div>
                            </button>
                        }
                    }).collect_view()}
                </div>
            </div>

            // ── Error region (local validation + server errors) ───────
            <div aria-live="polite" class="lns-error-banner">
                {move || {
                    local_error.get()
                        .or_else(|| server_error())
                        .map(|msg| view! { <p class="lns-error-banner-text">{msg}</p> })
                }}
            </div>

            // ── Submit button ─────────────────────────────────────────
            <button
                type="button"
                class="lns-btn lns-btn--primary lns-btn--full"
                disabled=move || is_pending()
                on:click=on_submit
            >
                {move || if is_pending() { "Creating…" } else { "Create board" }}
            </button>
        </Modal>
    }
}
