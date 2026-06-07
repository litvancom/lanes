use leptos::prelude::*;
use crate::models::BoardWithMeta;
use crate::api::workspace_api::ToggleStarBoard;

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
/// - Inert placeholders: Share, filter, labels toggle, overflow (Phase 4 activates)
///
/// Threat mitigations:
/// - T-03-17: `safe_hex` validates the board color before interpolating into inline style
/// - T-03-20: `toggle_star_board` server fn enforces board membership before the UPDATE
///
/// NOTE: 03-06 will add an Archive action to the overflow menu — the overflow-menu
/// structure is kept as a `<div class="lns-board-header-overflow">` placeholder
/// so 03-06 can populate it without structural rework.
#[component]
pub fn BoardHeader(board: BoardWithMeta) -> impl IntoView {
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

                // Filter (inert Phase 3)
                <button type="button" class="lns-btn lns-btn--ghost" disabled=true>
                    <crate::components::icon::Icon name="filter"/>
                    "Filter"
                </button>

                // Labels toggle (inert Phase 3)
                <button type="button" class="lns-btn lns-btn--ghost" disabled=true>
                    "Labels"
                </button>

                // Overflow menu placeholder — 03-06 will add Archive action here
                // Kept as a named div so 03-06 can populate without structural rework
                <div class="lns-board-header-overflow">
                    <button type="button" class="lns-icon-btn" disabled=true aria-label="Board options">
                        <crate::components::icon::Icon name="dots"/>
                    </button>
                </div>
            </div>
        </header>
    }
}
