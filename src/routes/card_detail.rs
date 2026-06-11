//! Card detail route: `/board/:id/card/:card_num`
//!
//! Mounts the `CardDetailModal` as an Outlet child of `BoardPage`.
//! On SSR: the board is rendered with the modal open (deep-linkable, D-01).
//! On client: navigating to the card URL opens the modal without a full reload.
//!
//! # Params
//! Both parent `:id` and child `:card_num` must be included in `CardDetailParams`
//! (Pitfall 2 from 05-RESEARCH.md: params merge across the full matched path).
//!
//! # Auth
//! `get_card_detail` enforces board membership; Err → Redirect to /.

use leptos::prelude::*;
use leptos_router::params::Params;
use leptos_router::hooks::use_params;
use leptos_router::components::Redirect;
use crate::api::card_detail_api::get_card_detail;
use crate::components::card_detail::CardDetailModal;

/// Route params for `/board/:id/card/:card_num`.
///
/// MUST include the parent route param `:id` alongside the child `:card_num`.
/// Leptos merges params from the full matched path, so both fields are accessible
/// via `use_params::<CardDetailParams>()` in the child route component.
#[derive(Params, PartialEq, Clone)]
struct CardDetailParams {
    id: Option<String>,
    card_num: Option<i64>,
}

/// Child route component for card detail modal.
///
/// Reads both `:id` (board_id from the parent `/board/:id` segment) and
/// `:card_num` (from `card/:card_num`), then SSR-prefetches the card detail
/// via `get_card_detail`. Renders `CardDetailModal` inside a `<Suspense>`.
#[component]
pub fn CardDetailRoute() -> impl IntoView {
    // --- Route params (both parent :id and child :card_num merged) ---
    let params = use_params::<CardDetailParams>();

    let board_id = move || {
        params.with(|p| {
            p.as_ref()
                .ok()
                .and_then(|p| p.id.clone())
                .unwrap_or_default()
        })
    };

    let card_num = move || {
        params.with(|p| {
            p.as_ref()
                .ok()
                .and_then(|p| p.card_num)
                .unwrap_or_default()
        })
    };

    // --- SSR prefetch for card detail data ---
    let detail_data = Resource::new(
        move || (board_id(), card_num()),
        |(bid, cn)| async move { get_card_detail(bid, cn).await },
    );

    view! {
        <Suspense fallback=move || view! {
            <div class="lns-modal-loading">"Loading card…"</div>
        }>
            {move || {
                detail_data.get().map(|result| {
                    match result {
                        Err(_) => view! { <Redirect path="/"/> }.into_any(),
                        Ok(_data) => view! {
                            <CardDetailModal
                                detail_data=detail_data
                                board_id=board_id()
                                card_num=card_num()
                            />
                        }.into_any(),
                    }
                })
            }}
        </Suspense>
    }
}
