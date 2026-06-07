use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use leptos_use::{use_event_listener, use_window, use_debounce_fn};
use crate::api::workspace_api::SearchBoards;
use crate::components::icon::Icon;
use crate::components::board_card::safe_hex;

/// Workspace topbar — 52px height.
///
/// Structure:
/// - Search field (min-width 320px, debounced 300ms, ⌘K chip, dropdown)
/// - Spacer
/// - Inert bell + users icon buttons (Phase 7)
/// - Avatar initial bubble (display_name first character)
/// - "New board" primary CTA button
///
/// Keyboard shortcuts:
/// - ⌘K / Ctrl+K: focus search field (D-14)
/// - Escape: close search dropdown
///
/// Search dropdown:
/// - Triggers after ≥1 char + 300ms debounce (Pitfall 4, T-03-26)
/// - role="listbox" + role="option" for accessibility (UI-SPEC)
/// - Navigate to /board/:id on click; close dropdown
/// - Parameterized server fn search_boards(query) — no client-side SQL (T-03-21)
///
/// Threat mitigations:
/// - T-03-21: search dispatches server fn with parameterized query
/// - T-03-22: board names escaped by Leptos view! (no inner_html)
/// - T-03-23: safe_hex() validates colors in dropdown chips
/// - T-03-26: 300ms debounce + min 1 char before dispatch
#[component]
pub fn WorkspaceTopbar(
    /// User's display name for the avatar bubble and greeting.
    #[prop(into)] display_name: String,
    /// Called when the "New board" button is clicked.
    on_new_board: Callback<()>,
) -> impl IntoView {
    // --- Search state ---
    let search_query = RwSignal::new(String::new());
    let show_dropdown = RwSignal::new(false);
    let search_ref = NodeRef::<leptos::html::Input>::new();

    // Navigate helper for dropdown click
    let navigate = use_navigate();
    let navigate_signal = StoredValue::new(navigate);

    // --- ServerAction for search ---
    let search_action = ServerAction::<SearchBoards>::new();

    // --- Debounced search dispatcher (300ms, min 1 char — Pitfall 4, T-03-26) ---
    let search_action_stored = StoredValue::new(search_action);
    let debounced_search = use_debounce_fn(
        move || {
            let q = search_query.get();
            let trimmed = q.trim().to_string();
            if trimmed.len() >= 1 {
                search_action_stored.get_value().dispatch(SearchBoards { query: trimmed });
                show_dropdown.set(true);
            } else {
                show_dropdown.set(false);
            }
        },
        300.0,
    );
    let debounced_stored = StoredValue::new(debounced_search);

    // --- ⌘K / Ctrl+K focus + Escape close (D-14, Pattern 2) ---
    let show_dropdown2 = show_dropdown;
    let search_ref2 = search_ref;
    let _ = use_event_listener(use_window(), leptos::ev::keydown, move |e: leptos::ev::KeyboardEvent| {
        if (e.meta_key() || e.ctrl_key()) && e.key() == "k" {
            e.prevent_default();
            if let Some(input) = search_ref2.get() {
                let _ = input.focus();
            }
        }
        if e.key() == "Escape" {
            show_dropdown2.set(false);
        }
    });

    // --- Avatar initial ---
    let initial = display_name.chars().next().unwrap_or('?').to_uppercase().to_string();

    // --- Search results derived from action value ---
    // When action returns Ok(boards), show them; on Err or None, hide dropdown
    let results = move || {
        search_action.value().get()
            .and_then(|r| r.ok())
            .unwrap_or_default()
    };

    let has_results = move || !results().is_empty();

    view! {
        <header class="lns-topbar">
            // --- Search field ---
            <div class="lns-search-wrap">
                <input
                    node_ref=search_ref
                    type="text"
                    class="lns-search"
                    placeholder="Search boards, cards, comments\u{2026}"
                    on:input=move |ev| {
                        let val = event_target_value(&ev);
                        search_query.set(val.clone());
                        if val.trim().is_empty() {
                            show_dropdown.set(false);
                        } else {
                            debounced_stored.get_value()();
                        }
                    }
                    on:focus=move |_| {
                        if !search_query.get().trim().is_empty() {
                            show_dropdown.set(true);
                        }
                    }
                />
                <span class="lns-kbd">"⌘K"</span>

                // --- Search dropdown ---
                <Show when=move || show_dropdown.get() && has_results()>
                    <div class="lns-search-dropdown" role="listbox">
                        <span class="lns-search-group-label">"Boards"</span>
                        <For
                            each=move || results()
                            key=|b| b.id.clone()
                            children={
                                let navigate_signal = navigate_signal;
                                move |board| {
                                    let c = safe_hex(&board.color);
                                    let chip_style = format!(
                                        "background:{c};",
                                    );
                                    let href = format!("/board/{}", board.id);
                                    let nav = navigate_signal.get_value();
                                    view! {
                                        <button
                                            type="button"
                                            class="lns-search-result"
                                            role="option"
                                            on:click={
                                                let href = href.clone();
                                                move |_| {
                                                    show_dropdown.set(false);
                                                    nav(&href, Default::default());
                                                }
                                            }
                                        >
                                            <span class="lns-search-chip" style=chip_style/>
                                            <span class="lns-search-result-name">{board.name.clone()}</span>
                                            <span class="lns-search-result-key">{board.key_prefix.clone()}</span>
                                        </button>
                                    }
                                }
                            }
                        />
                    </div>
                </Show>
            </div>

            // --- Spacer ---
            <div class="lns-topbar-spacer"/>

            // --- Right side actions ---
            <div class="lns-topbar-actions">
                // Inert bell (Phase 7)
                <button type="button" class="lns-icon-btn" aria-label="Notifications" disabled>
                    <Icon name="bell"/>
                </button>
                // Inert users (Phase 7)
                <button type="button" class="lns-icon-btn" aria-label="Members" disabled>
                    <Icon name="users"/>
                </button>
                // Avatar bubble
                <div
                    class="lns-avatar-bubble"
                    style="background: var(--text); color: var(--text-inverse);"
                    aria-label=format!("Account: {}", display_name)
                >
                    {initial}
                </div>
                // "New board" primary CTA
                <button
                    type="button"
                    class="lns-btn lns-btn--primary"
                    on:click=move |_| on_new_board.run(())
                >
                    "New board"
                </button>
            </div>
        </header>
    }
}
