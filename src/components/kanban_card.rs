// Placeholder — full implementation in Task 2
use leptos::prelude::*;
use crate::models::Card;

#[component]
pub fn KanbanCard(
    card: RwSignal<Card>,
    labels_expanded: RwSignal<bool>,
    list_id: String,
) -> impl IntoView {
    let _ = card;
    let _ = labels_expanded;
    let _ = list_id;
    view! { <div class="lns-card"/> }
}
