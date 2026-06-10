use leptos::prelude::*;
use crate::models::BoardWithMeta;
use crate::api::workspace_api::{ToggleStarBoard, ArchiveBoard};

/// Validate a board color as a 6-digit hex (`#rrggbb`).
/// Returns a safe default rather than interpolating untrusted-shaped data
/// into a CSS string (T-03-17 Tampering mitigation — matches board_card.rs defense).
fn safe_hex(c: &str) -> &str {
    let ok = c.len() == 7
        && c.starts_with('#')
        && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
    if ok { c } else { "#7c5cff" }
}

/// Board header (design screen 04).
///
/// Renders:
/// - Breadcrumb "Boards ›" (link to /)
/// - Board title (17px/700) with a 14×14px solid color chip (radius 4px, T-03-17)
/// - Star toggle button (optimistic local state, dispatches ToggleStarBoard)
/// - Inert placeholders: Share, filter, labels toggle
/// - Overflow menu with Archive action (inline-confirm pattern, UI-SPEC §Archive Confirmation)
///
/// Threat mitigations:
/// - T-03-17: `safe_hex` validates the board color before interpolating into inline style
/// - T-03-20: `toggle_star_board` server fn enforces board membership before the UPDATE
/// - T-03-28: archive_board server fn enforces owner-only check regardless of UI render
#[component]
pub fn BoardHeader(
    board: BoardWithMeta,
    /// Search/filter signal from BoardSignals (CARD-05)
    search: RwSignal<String>,
    /// Label expand/collapse signal from BoardSignals (CARD-06)
    labels_expanded: RwSignal<bool>,
) -> impl IntoView {
    // Validate the board color defensively (T-03-17)
    let validated_color = safe_hex(&board.color).to_string();

    // Local starred state — seeded from the board's per-user flag, flipped optimistically
    let starred = RwSignal::new(board.starred);

    // Star server action
    let star_action = ServerAction::<ToggleStarBoard>::new();

    // Optimistically flip the star on dispatch; the server fn persists it
    let board_id_for_star = board.id.clone();
    let on_star_click = move |e: leptos::ev::MouseEvent| {
        e.stop_propagation();
        // Optimistic flip
        starred.update(|v| *v = !*v);
        star_action.dispatch(ToggleStarBoard {
            board_id: board_id_for_star.clone(),
        });
    };

    let board_name = board.name.clone();

    // ── Archive overflow action (inline-confirm, UI-SPEC §Archive Confirmation) ──
    // Inline-confirm state: false = show "Archive", true = show "Confirm archive" + Cancel
    let confirming_archive = RwSignal::new(false);
    // Archive server action — on success navigate to workspace home
    let archive_action = ServerAction::<ArchiveBoard>::new();
    // Use StoredValue so the board_id can be cloned inside Fn (not FnOnce) closures
    let board_id_sv = StoredValue::new(board.id.clone());

    // Overflow menu open/close state
    let overflow_open = RwSignal::new(false);

    // Navigate to / after archive succeeds (archived board disappears from grid)
    Effect::new(move |_| {
        if matches!(archive_action.value().get(), Some(Ok(_))) {
            // Navigate to workspace home — the archived board is now gone from the grid
            let _ = leptos_router::hooks::use_navigate()(
                "/",
                leptos_router::NavigateOptions::default(),
            );
        }
    });

    view! {
        <header class="lns-board-header">
            // ── Breadcrumb ────────────────────────────────────────────────
            <div class="lns-board-breadcrumb">
                <a href="/" class="lns-board-breadcrumb-link">"Boards"</a>
                <span class="lns-board-breadcrumb-sep" aria-hidden="true">"›"</span>
            </div>

            // ── Title + color chip ─────────────────────────────────────────
            <div class="lns-board-title-group">
                <span
                    class="lns-board-color-chip"
                    style=format!("background-color: {};", validated_color)
                    aria-hidden="true"
                />
                <h1 class="lns-board-title">{board_name}</h1>
            </div>

            // ── Star toggle ─────────────────────────────────────────────────
            <button
                type="button"
                class=move || {
                    if starred.get() {
                        "lns-star-btn lns-star-btn--starred"
                    } else {
                        "lns-star-btn"
                    }
                }
                aria-label=move || {
                    if starred.get() { "Unstar board" } else { "Star board" }
                }
                on:click=on_star_click
            >
                {move || if starred.get() {
                    view! { <crate::components::icon::Icon name="star-filled"/> }.into_any()
                } else {
                    view! { <crate::components::icon::Icon name="star"/> }.into_any()
                }}
            </button>

            // ── Spacer ───────────────────────────────────────────────────────
            <div class="lns-board-header-spacer"/>

            // ── Inert placeholders (Phase 4 activates filter + labels) ───────
            <div class="lns-board-header-actions">
                // Share button (inert Phase 3)
                <button type="button" class="lns-btn lns-btn--ghost" disabled=true>
                    "Share"
                </button>

                // Filter search input (Phase 4 activated — CARD-05)
                <div class="lns-search">
                    <crate::components::icon::Icon name="search"/>
                    <input
                        type="text"
                        placeholder="Filter cards…"
                        prop:value=move || search.get()
                        on:input=move |ev| search.set(event_target_value(&ev))
                        aria-label="Filter cards"
                    />
                    <Show when=move || !search.get().is_empty()>
                        <button
                            type="button"
                            class="lns-icon-btn"
                            aria-label="Clear filter"
                            on:click=move |_| search.set(String::new())
                        >
                            <crate::components::icon::Icon name="x"/>
                        </button>
                    </Show>
                </div>

                // Labels toggle (Phase 4 activated — CARD-06)
                <button
                    type="button"
                    class=move || {
                        if labels_expanded.get() {
                            "lns-btn lns-btn--sm lns-btn--primary"
                        } else {
                            "lns-btn lns-btn--sm"
                        }
                    }
                    on:click=move |_| labels_expanded.update(|v| *v = !*v)
                >
                    <crate::components::icon::Icon name="tag"/>
                    "Labels"
                </button>

                // Overflow menu — Archive action with inline-confirm (UI-SPEC §Archive Confirmation)
                <div class="lns-board-header-overflow">
                    // Overflow dots button — toggles the dropdown
                    <button
                        type="button"
                        class="lns-icon-btn"
                        aria-label="Board options"
                        aria-expanded=move || overflow_open.get().to_string()
                        on:click=move |_| {
                            overflow_open.update(|v| *v = !*v);
                            // Reset inline-confirm when reopening
                            confirming_archive.set(false);
                        }
                    >
                        <crate::components::icon::Icon name="dots"/>
                    </button>

                    // Dropdown menu — shown when overflow_open is true
                    <Show when=move || overflow_open.get()>
                        <div class="lns-overflow-menu" role="menu">
                            // Archive item with inline-confirm (UI-SPEC §Archive Confirmation)
                            <Show
                                when=move || !confirming_archive.get()
                                fallback=move || {
                                    // Confirming state: "Confirm archive" + "Cancel"
                                    view! {
                                        <div class="lns-overflow-menu-confirm">
                                            <button
                                                type="button"
                                                class="lns-overflow-item lns-overflow-item--danger"
                                                role="menuitem"
                                                on:click=move |_| {
                                                    archive_action.dispatch(ArchiveBoard {
                                                        board_id: board_id_sv.get_value(),
                                                    });
                                                    overflow_open.set(false);
                                                    confirming_archive.set(false);
                                                }
                                            >
                                                "Confirm archive"
                                            </button>
                                            <button
                                                type="button"
                                                class="lns-overflow-item"
                                                role="menuitem"
                                                on:click=move |_| {
                                                    confirming_archive.set(false);
                                                }
                                            >
                                                "Cancel"
                                            </button>
                                        </div>
                                    }
                                }
                            >
                                // Default state: "Archive" item
                                <button
                                    type="button"
                                    class="lns-overflow-item"
                                    role="menuitem"
                                    on:click=move |_| {
                                        // Enter inline-confirm mode (UI-SPEC: button → "Confirm archive" + Cancel)
                                        confirming_archive.set(true);
                                    }
                                >
                                    <crate::components::icon::Icon name="archive"/>
                                    "Archive"
                                </button>
                            </Show>
                        </div>
                    </Show>
                </div>
            </div>
        </header>
    }
}
