use leptos::prelude::*;
use crate::models::CardDetail;

/// Card detail modal shell.
///
/// Stub — implemented in Plan 02 Task 2.
#[component]
pub fn CardDetailModal(
    detail_data: Resource<Result<CardDetail, ServerFnError>>,
    board_id: String,
    card_num: i64,
) -> impl IntoView {
    let _ = detail_data;
    let _ = board_id;
    let _ = card_num;
    view! { <></> }
}
