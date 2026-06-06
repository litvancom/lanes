use leptos::prelude::*;
use crate::models::Board;

/// A board card for the workspace home grid.
/// Design spec (screen 02): white card, border 1px --border, radius --radius-md.
/// Header band 60px with gradient. Body: board name 14px/700, updated meta line.
#[component]
pub fn BoardCard(board: Board) -> impl IntoView {
    // Header gradient using the board color with alpha suffix (design spec)
    let gradient = format!(
        "linear-gradient(135deg, {}33, {}11)",
        board.color, board.color
    );

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
