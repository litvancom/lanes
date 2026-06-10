// Placeholder — full implementation in Task 2
use leptos::prelude::*;
use crate::models::CardLabel;

#[component]
pub fn LabelChip(
    label: CardLabel,
    expanded: RwSignal<bool>,
) -> impl IntoView {
    let _ = label;
    let _ = expanded;
    view! { <span class="lns-label"/> }
}
