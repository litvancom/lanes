//! Board view page: `/board/:id`
//!
//! SSR-prefetches board data (header + lists + minimal card stubs) via `get_board`.
//! Shows EmptyBoardCard first-run state when the board has zero lists.
//! Wires inline list-create, list-rename, list-reorder composers.
//! Fires `touch_last_viewed` client-side after hydration (Pitfall 6 — never blocks SSR).
//!
//! # Auth
//! Non-members and unauthenticated users receive a `Redirect` to "/" (D-12 generic error).
//!
//! # Security
//! - T-03-16: get_board enforces board membership; Err → redirect to /
//! - T-03-19: touch_last_viewed dispatched only from Effect (client-side after hydration)

use leptos::prelude::*;
use leptos_router::params::Params;
use leptos_router::hooks::use_params;
use leptos_router::components::Redirect;
use crate::api::board_api::{get_board, TouchLastViewed};
use crate::api::list_api::{CreateList, RenameList, ReorderList};
use crate::components::board_header::BoardHeader;
use crate::components::kanban_list::{KanbanList, AddListComposer, EmptyBoardCard};
use crate::components::create_board_modal::CreateBoardModal;

/// Route params for `/board/:id`.
#[derive(Params, PartialEq, Clone)]
struct BoardParams {
    id: Option<String>,
}

/// Board view page component (`/board/:id`).
///
/// SSR flow:
/// 1. Parse `:id` from route params
/// 2. `Resource::new` calls `get_board` during SSR (read-only, Pitfall 6)
/// 3. `<Suspense>` wraps the render; non-member/not-found → Redirect to /
/// 4. Member → BoardHeader + lists canvas or EmptyBoardCard
///
/// Client flow (after hydration):
/// - `Effect::new` dispatches `TouchLastViewed` (Pitfall 6: not during SSR)
/// - List mutations (create/rename/reorder) trigger `board_data.refetch()`
#[component]
pub fn BoardPage() -> impl IntoView {
    // --- Route params ---
    let params = use_params::<BoardParams>();
    let board_id = move || {
        params.with(|p| {
            p.as_ref()
                .ok()
                .and_then(|p| p.id.clone())
                .unwrap_or_default()
        })
    };

    // --- SSR prefetch (read-only — does NOT write last_viewed_at, Pitfall 6) ---
    let board_data = Resource::new(board_id, |id| async move {
        get_board(id).await
    });

    // --- Touch last_viewed: client-side only via Effect (Pitfall 6 mitigation) ---
    // Effect runs after hydration; never during SSR. get_board is read-only.
    let touch = ServerAction::<TouchLastViewed>::new();
    Effect::new(move |_| {
        let id = board_id();
        if !id.is_empty() {
            touch.dispatch(TouchLastViewed { board_id: id });
        }
    });

    // --- List mutation actions owned by this route ---
    let create_list_action = ServerAction::<CreateList>::new();
    let rename_list_action = ServerAction::<RenameList>::new();
    let reorder_list_action = ServerAction::<ReorderList>::new();

    // Refetch board_data when any list mutation succeeds
    Effect::new(move |_| {
        if matches!(create_list_action.value().get(), Some(Ok(_))) {
            board_data.refetch();
        }
    });
    Effect::new(move |_| {
        if matches!(rename_list_action.value().get(), Some(Ok(_))) {
            board_data.refetch();
        }
    });
    Effect::new(move |_| {
        if matches!(reorder_list_action.value().get(), Some(Ok(_))) {
            board_data.refetch();
        }
    });

    // --- CreateBoardModal signal (for EmptyBoardCard "Browse templates" path) ---
    let show_create_modal = RwSignal::new(false);

    // Signal to open the AddListComposer from EmptyBoardCard "Add your first list"
    let open_composer = RwSignal::new(false);

    view! {
        <div class="lns-app-shell lns-board">
            <Suspense fallback=move || view! {
                <div class="lns-board-loading">"Loading board…"</div>
            }>
                {move || {
                    board_data.get().map(|result| {
                        match result {
                            // Non-member or board not found → redirect to / (D-12, T-03-16)
                            Err(_) => view! {
                                <Redirect path="/"/>
                            }.into_any(),

                            Ok(data) => {
                                let board_clone = data.board.clone();
                                let lists = data.lists;
                                let cards = data.cards;
                                let lists_empty = lists.is_empty();

                                // Store board_id so closures can share it without move conflicts
                                let board_id_sv = StoredValue::new(data.board.id.clone());

                                // Store lists/cards for use in multiple closures
                                let lists_sv = StoredValue::new(lists);
                                let cards_sv = StoredValue::new(cards);

                                view! {
                                    // Board header (breadcrumb + title + color chip + star)
                                    <BoardHeader board=board_clone/>

                                    // Board canvas — horizontal scroll, gap 12px, padding 16px 20px
                                    <div class="lns-board-canvas">
                                        <Show
                                            when=move || lists_empty && !open_composer.get()
                                            fallback=move || {
                                                // Non-empty board: render KanbanList × N + AddListComposer
                                                let lists_for_render = lists_sv.get_value();
                                                let cards_for_render = cards_sv.get_value();
                                                let n = lists_for_render.len();

                                                view! {
                                                    {lists_for_render.into_iter().enumerate().map(|(idx, list)| {
                                                        let list_cards: Vec<crate::models::Card> = cards_for_render.iter()
                                                            .filter(|c| c.list_id == list.id)
                                                            .cloned()
                                                            .collect();

                                                        // Compute neighbor positions for Move left/right (Pattern 4, D-15)
                                                        let can_move_left = idx > 0;
                                                        let can_move_right = idx < n - 1;

                                                        let lists_snap = lists_sv.get_value();

                                                        // left_neighbor_pos: the list immediately to the left
                                                        let left_neighbor_pos = if idx > 0 {
                                                            Some(lists_snap[idx - 1].position.clone())
                                                        } else {
                                                            None
                                                        };

                                                        // left_left_neighbor_pos: the list two slots to the left
                                                        let left_left_neighbor_pos = if idx >= 2 {
                                                            Some(lists_snap[idx - 2].position.clone())
                                                        } else {
                                                            None
                                                        };

                                                        // right_neighbor_pos: the list immediately to the right
                                                        let right_neighbor_pos = if idx < n - 1 {
                                                            Some(lists_snap[idx + 1].position.clone())
                                                        } else {
                                                            None
                                                        };

                                                        // right_right_neighbor_pos: two slots to the right
                                                        let right_right_neighbor_pos = if idx + 2 < n {
                                                            Some(lists_snap[idx + 2].position.clone())
                                                        } else {
                                                            None
                                                        };

                                                        view! {
                                                            <KanbanList
                                                                list=list
                                                                cards=list_cards
                                                                can_move_left=can_move_left
                                                                can_move_right=can_move_right
                                                                left_neighbor_pos=left_neighbor_pos
                                                                left_left_neighbor_pos=left_left_neighbor_pos
                                                                right_neighbor_pos=right_neighbor_pos
                                                                right_right_neighbor_pos=right_right_neighbor_pos
                                                                rename_action=rename_list_action
                                                                reorder_action=reorder_list_action
                                                            />
                                                        }
                                                    }).collect_view()}

                                                    // Add another list composer
                                                    <AddListComposer
                                                        board_id=board_id_sv.get_value()
                                                        create_action=create_list_action
                                                    />
                                                }.into_any()
                                            }
                                        >
                                            // Empty board first-run state (BOARD-01, design screen 03)
                                            <EmptyBoardCard
                                                on_add_list=Callback::new(move |_| open_composer.set(true))
                                                on_browse_templates=Callback::new(move |_| show_create_modal.set(true))
                                            />
                                        </Show>

                                        // When open_composer is set from EmptyBoardCard, show AddListComposer
                                        <Show when=move || open_composer.get() && lists_empty>
                                            <AddListComposer
                                                board_id=board_id_sv.get_value()
                                                create_action=create_list_action
                                            />
                                        </Show>
                                    </div>

                                    // CreateBoardModal — mounted once for browse-templates path
                                    <CreateBoardModal show=show_create_modal/>
                                }.into_any()
                            }
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}
