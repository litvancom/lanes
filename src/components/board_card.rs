use leptos::prelude::*;
use crate::models::Board;

/// A board card for the workspace home grid.
/// Design spec (screen 02): white card, border 1px --border, radius --radius-md.
/// Header band 60px with gradient. Body: board name 14px/700, updated meta line.
/// Validate a board color as a 6-digit hex (`#rrggbb`). Returns a safe default
/// rather than interpolating untrusted-shaped data into a CSS string (CR-01).
fn safe_hex(c: &str) -> &str {
    let ok = c.len() == 7
        && c.starts_with('#')
        && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
    if ok { c } else { "#7c5cff" }
}

#[component]
pub fn BoardCard(board: Board) -> impl IntoView {
    // Header gradient using the board color with alpha suffix (design spec).
    // Defensively validate the color so a free-form TEXT value can never inject
    // into the inline CSS `style` attribute (CR-01 — stored CSS-injection sink).
    let c = safe_hex(&board.color);
    let gradient = format!("linear-gradient(135deg, {c}33, {c}11)");

    view! {
        <div class="board-card">
            <div class="board-card-header" style=gradient/>
            <div class="board-card-body">
                <p class="board-card-name">{board.name.clone()}</p>
                <p class="board-card-meta">
                    {board.key_prefix.clone()}
                </p>
            </div>
        </div>
    }
}
