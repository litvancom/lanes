use leptos::prelude::*;
use crate::models::TodayCard;

/// Today strip component — a white card listing due-today and overdue cards.
///
/// Rendered only when `cards` is non-empty (D-04: parent controls visibility via
/// `<Show when=|| !today_cards.is_empty()>`).
///
/// Each row links to `/board/{board_id}` (D-03; Phase 5 upgrades to open card modal).
///
/// Row layout:
/// - 8px status dot: --danger (overdue) or --warning (due today)
/// - Card title: 13px/500, --text
/// - Board name: 12px/400, --text-muted
/// - Due pill: 11px/600, --danger or --warning
///
/// Threat mitigations:
/// - T-03-22: All text nodes escaped by Leptos view! (no inner_html)
/// - T-03-24: Server fn scopes results to current user's boards
#[component]
pub fn TodayStrip(cards: Vec<TodayCard>) -> impl IntoView {
    view! {
        <div class="lns-today-strip">
            <For
                each=move || cards.clone()
                key=|c| c.id.clone()
                children=|card| {
                    let dot_class = if card.overdue {
                        "lns-today-dot lns-today-dot--danger"
                    } else {
                        "lns-today-dot lns-today-dot--warning"
                    };
                    let pill_class = if card.overdue {
                        "lns-today-pill lns-today-pill--danger"
                    } else {
                        "lns-today-pill lns-today-pill--warning"
                    };
                    let due_label = if card.overdue { "Overdue" } else { "Due today" };
                    let href = format!("/board/{}", card.board_id);
                    view! {
                        <a href=href class="lns-today-row">
                            <span class=dot_class/>
                            <span class="lns-today-title">{card.title.clone()}</span>
                            <span class="lns-today-board">{card.board_name.clone()}</span>
                            <span class=pill_class>{due_label}</span>
                        </a>
                    }
                }
            />
        </div>
    }
}
