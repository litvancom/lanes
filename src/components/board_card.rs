use leptos::prelude::*;
use crate::models::BoardWithMeta;
use crate::components::icon::Icon;

/// Validate a board color as a 6-digit hex (`#rrggbb`). Returns a safe default
/// rather than interpolating untrusted-shaped data into a CSS string (T-03-23).
pub fn safe_hex(c: &str) -> &str {
    let ok = c.len() == 7
        && c.starts_with('#')
        && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
    if ok { c } else { "#7c5cff" }
}

/// Board card component for the workspace home grid.
///
/// Design spec (screen 02): white card, border 1px --border, radius --radius-md.
/// Header band: 60px (regular) or 100px (large/recent variant) with gradient.
/// Hover-revealed star button (28x28, opacity 0→1, transition 0.12s) — stop_propagation
/// prevents the star click from also navigating to the board (Pitfall 5).
/// Body: board name (14px/700), card count meta (12px --text-muted).
/// Wrapped in an <a> link to /board/:id for keyboard/click navigation.
///
/// Threat mitigations:
/// - T-03-23: safe_hex() validates color before CSS gradient interpolation
/// - T-03-25: stop_propagation on star click prevents accidental nav
/// - T-03-22: Leptos view! escapes all text nodes (no inner_html)
#[component]
pub fn BoardCard(
    board: BoardWithMeta,
    /// Optional callback invoked with the board ID when the user clicks the star.
    /// If None, no star button is rendered.
    #[prop(optional)] on_star: Option<Callback<String>>,
    /// When true, renders the 100px header band (for the "Recently viewed" 3-col grid).
    /// Default (false) renders the standard 60px header.
    #[prop(optional)] large: bool,
) -> impl IntoView {
    // Header gradient: board color with alpha suffixes (design spec, T-03-23).
    let c = safe_hex(&board.color);
    let gradient = format!("linear-gradient(135deg, {c}33, {c}11)");
    // Inline chip color style (solid, 14x14px square)
    let chip_style = format!("background:{c};width:14px;height:14px;border-radius:4px;flex-shrink:0;");

    let board_id = board.id.clone();
    let board_id_for_star = board.id.clone();
    let board_name = board.name.clone();
    let starred = board.starred;
    let card_count = board.card_count;

    // Card header class switches height based on the `large` prop
    let header_class = if large { "board-card-header board-card-header--large" } else { "board-card-header" };

    view! {
        <div class="board-card">
            // Full-card link for keyboard and click navigation to the board
            <a href=format!("/board/{}", board_id) class="board-card-link">
                <div class="board-card-header-wrap">
                    <div class=header_class style=gradient>
                        // Color chip (top-right of header band)
                        <div class="board-card-chip" style=chip_style/>
                        // Hover-revealed star button (top-left of header band) — Pitfall 5
                        {move || {
                            if let Some(cb) = on_star.clone() {
                                let id = board_id_for_star.clone();
                                let icon_name = if starred { "star-filled" } else { "star" };
                                let aria_label = if starred { "Unstar board" } else { "Star board" };
                                view! {
                                    <button
                                        class="board-card-star"
                                        aria-label=aria_label
                                        on:click=move |e: leptos::ev::MouseEvent| {
                                            // stop_propagation prevents the <a> link from navigating
                                            e.stop_propagation();
                                            cb.run(id.clone());
                                        }
                                    >
                                        <Icon name=icon_name/>
                                    </button>
                                }.into_any()
                            } else {
                                ().into_any()
                            }
                        }}
                    </div>
                </div>
                <div class="board-card-body">
                    <p class="board-card-name">{board_name}</p>
                    <p class="board-card-meta">
                        {format!("{} card{}", card_count, if card_count == 1 { "" } else { "s" })}
                    </p>
                </div>
            </a>
        </div>
    }
}
