use leptos::prelude::*;
use crate::models::BoardWithMeta;
use crate::api::workspace_api::{ToggleStarBoard, ArchiveBoard};
use crate::api::board_api::RenameBoard;
use crate::components::presence_stack::PresenceStack;

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
///   - Owner: inline-editable (click to enter edit mode, Enter/blur saves, Esc cancels)
///   - Non-owner: static display
/// - Star toggle button (optimistic local state, dispatches ToggleStarBoard)
/// - Owner-only Share button that opens the ShareModal
/// - Filter search input and labels toggle
/// - Overflow menu with Archive action (inline-confirm pattern, UI-SPEC §Archive Confirmation)
///
/// Threat mitigations:
/// - T-03-17: `safe_hex` validates the board color before interpolating into inline style
/// - T-03-20: `toggle_star_board` server fn enforces board membership before the UPDATE
/// - T-03-28: archive_board server fn enforces owner-only check regardless of UI render
/// - T-naw-01: rename_board server fn enforces owner-only independently of is_owner UI gate
/// - T-naw-04: Leptos view! text interpolation escapes board name (no inner_html)
#[component]
pub fn BoardHeader(
    board: BoardWithMeta,
    /// Search/filter signal from BoardSignals (CARD-05)
    search: RwSignal<String>,
    /// Label expand/collapse signal from BoardSignals (CARD-06)
    labels_expanded: RwSignal<bool>,
    /// True when the current viewer holds the owner role on this board.
    /// Controls visibility of the Share button and inline-rename affordance.
    is_owner: bool,
    /// Reactive board name — seeded from SSR data, updated live by BoardRenamed WS events.
    /// Passed from board.rs BoardSignals so the header re-renders on remote renames.
    board_name: RwSignal<String>,
) -> impl IntoView {
    // Validate the board color defensively (T-03-17)
    let validated_color = safe_hex(&board.color).to_string();

    // Local starred state — seeded from the board's per-user flag, flipped optimistically
    let starred = RwSignal::new(board.starred);

    // Star server action
    let star_action = ServerAction::<ToggleStarBoard>::new();

    // Share modal open/close state (owner-only)
    let show_share = RwSignal::new(false);
    // board_id for the share modal — cloned before other board.id clones consume board
    let board_id_for_modal = board.id.clone();

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

    // ── Owner-only inline rename ──────────────────────────────────────────────
    // Mirrors kanban_list.rs's inline-rename pattern exactly.
    let editing = RwSignal::new(false);
    let title_input_ref = NodeRef::<leptos::html::Input>::new();

    // Auto-focus the input when entering edit mode (Pattern 3, mirrors kanban_list focus Effect)
    Effect::new(move |_| {
        if editing.get() {
            if let Some(input) = title_input_ref.get() {
                let _ = input.focus();
            }
        }
    });

    // RenameBoard server action — dispatched on Enter/blur when name changed and non-empty
    let rename_action = ServerAction::<RenameBoard>::new();

    // Store board.id in a StoredValue so commit_rename closure stays Fn (not FnOnce)
    // Mirrors kanban_list.rs list_id_sv pattern exactly.
    let rename_board_id_sv = StoredValue::new(board.id.clone());

    // Commit rename closure — trims, compares, dispatches if changed & non-empty, always exits edit
    let commit_rename = move || {
        if let Some(input) = title_input_ref.get() {
            let new_name = input.value();
            let trimmed = new_name.trim().to_string();
            let current = board_name.get_untracked();
            if !trimmed.is_empty() && trimmed != current {
                // Optimistic update — server fn publishes BoardRenamed which echoes back
                // and sets board_name again (harmless idempotent set).
                board_name.set(trimmed.clone());
                rename_action.dispatch(RenameBoard {
                    board_id: rename_board_id_sv.get_value(),
                    name: trimmed,
                    client_id: use_context::<crate::routes::board::BoardSignals>()
                        .and_then(|bs| bs.own_client_id.get_untracked())
                        .unwrap_or_default(),
                });
            }
        }
        editing.set(false);
    };

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
                // Owner: inline-editable title; Non-owner: static reactive title
                {if is_owner {
                    view! {
                        <Show
                            when=move || editing.get()
                            fallback=move || view! {
                                <h1
                                    class="lns-board-title"
                                    on:click=move |_| editing.set(true)
                                >
                                    {move || board_name.get()}
                                </h1>
                            }
                        >
                            <input
                                node_ref=title_input_ref
                                class="lns-board-title-input"
                                type="text"
                                prop:value=move || board_name.get_untracked()
                                on:keydown={
                                    let commit = commit_rename.clone();
                                    move |e: leptos::ev::KeyboardEvent| {
                                        match e.key().as_str() {
                                            "Enter" => commit(),
                                            "Escape" => editing.set(false),
                                            _ => {}
                                        }
                                    }
                                }
                                on:blur={
                                    let commit = commit_rename.clone();
                                    move |_| commit()
                                }
                            />
                        </Show>
                    }.into_any()
                } else {
                    view! {
                        <h1 class="lns-board-title">{move || board_name.get()}</h1>
                    }.into_any()
                }}
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

            // ── Presence avatar stack (06-04, RT-03 SC5) ─────────────────────
            // Current viewers excluding self. Reads BoardSignals.viewers from context.
            <PresenceStack/>

            // ── Actions row ────────────────────────────────────────────────
            <div class="lns-board-header-actions">
                // Share button — owner only; opens ShareModal
                {is_owner.then(|| view! {
                    <button
                        type="button"
                        class="lns-btn lns-btn--ghost"
                        on:click=move |_| show_share.set(true)
                    >
                        "Share"
                    </button>
                })}

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

            // Share modal — mounted once; controlled by show_share signal (owner-only)
            <crate::components::share_modal::ShareModal
                board_id=board_id_for_modal
                show=show_share
            />
        </header>
    }
}
