//! Board view page: `/board/:id`
//!
//! SSR-prefetches board data (header + lists + enriched card thumbnails) via `get_board`.
//! Shows EmptyBoardCard first-run state when the board has zero lists.
//! Wires inline list-create, list-rename, list-reorder, card-create composers.
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
use std::collections::{HashMap, HashSet};
use crate::models::Card;
use crate::api::board_api::{get_board, TouchLastViewed};
use crate::api::list_api::{CreateList, RenameList, ReorderList};
use crate::api::card_api::MoveCard;
use crate::components::board_header::BoardHeader;
use crate::components::kanban_list::{KanbanList, AddListComposer, EmptyBoardCard};
use crate::components::create_board_modal::CreateBoardModal;

/// Route params for `/board/:id`.
#[derive(Params, PartialEq, Clone)]
struct BoardParams {
    id: Option<String>,
}

/// Drag state — defined here so the context type is stable for Plan 03 (D-07 seam).
/// Plan 03 fills in the pointer-events logic; Plan 02 defines the struct.
#[derive(Clone, Debug)]
pub struct DragInfo {
    pub card_id: String,
    pub from_list_id: String,
    pub pointer_id: i32,
    pub start_x: f64,
    pub start_y: f64,
    pub current_x: f64,
    pub current_y: f64,
    pub is_dragging: bool,
}

/// Board-level reactive signals provided to all child components via context (D-05).
///
/// Replaces the Phase 3 `StoredValue` snapshot approach for cards.
/// Per-card `RwSignal<Card>` enables fine-grained updates without full re-renders.
/// `done_list_ids` is REQUIRED for Plan 03 commit_drop (D-06: auto-done on move to done list).
#[derive(Clone, Copy)]
pub struct BoardSignals {
    /// Ordered list IDs (source of truth for list render order)
    pub list_order: RwSignal<Vec<String>>,
    /// Per-list ordered card ID vecs
    pub list_cards: RwSignal<HashMap<String, Vec<String>>>,
    /// Per-card signals (card_id → RwSignal<Card>)
    pub card_signals: RwSignal<HashMap<String, RwSignal<Card>>>,
    /// IDs of lists with is_done_list=true (Plan 03 reads this to set done=true on drop)
    pub done_list_ids: RwSignal<HashSet<String>>,
    /// Current drag state (None = no drag in progress)
    pub drag_info: RwSignal<Option<DragInfo>>,
    /// List being hovered during a drag (for .drag-over highlight)
    pub hover_list_id: RwSignal<Option<String>>,
    /// Card being hovered over (the dragged card will be inserted before this card; None = append)
    pub before_card_id: RwSignal<Option<String>>,
    /// Filter search text (CARD-05)
    pub search: RwSignal<String>,
    /// Label expand/collapse toggle (CARD-06)
    pub labels_expanded: RwSignal<bool>,
    /// Server action for moving cards (owned here; KanbanList dispatches via BoardSignals)
    pub move_card_action: ServerAction<MoveCard>,
    /// Board ID (needed by KanbanList commit_drop to dispatch MoveCard)
    pub board_id: RwSignal<String>,
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
/// - Card creates update `BoardSignals` optimistically (no refetch needed)
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

    // Refetch board_data when any list mutation succeeds (lists still use Phase 3 refetch model)
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

                                // ── Build BoardSignals from SSR data ─────────────────────────
                                // Per-card signals (id → RwSignal<Card>)
                                let card_signals_map: HashMap<String, RwSignal<Card>> = cards.iter()
                                    .map(|c| (c.id.clone(), RwSignal::new(c.clone())))
                                    .collect();

                                // Per-list card ID vecs (preserving position order)
                                let list_cards_map: HashMap<String, Vec<String>> = lists.iter()
                                    .map(|l| {
                                        let mut list_card_ids: Vec<_> = cards.iter()
                                            .filter(|c| c.list_id == l.id)
                                            .collect();
                                        // Sort by fractional position so order matches DB
                                        list_card_ids.sort_by(|a, b| a.position.cmp(&b.position));
                                        let ids: Vec<String> = list_card_ids.iter().map(|c| c.id.clone()).collect();
                                        (l.id.clone(), ids)
                                    })
                                    .collect();

                                // List order (sorted by position)
                                let mut sorted_lists = lists.clone();
                                sorted_lists.sort_by(|a, b| a.position.cmp(&b.position));
                                let list_order: Vec<String> = sorted_lists.iter().map(|l| l.id.clone()).collect();

                                // Done list IDs (for Plan 03 commit_drop D-06)
                                let done_list_ids_set: HashSet<String> = lists.iter()
                                    .filter(|l| l.is_done_list)
                                    .map(|l| l.id.clone())
                                    .collect();

                                let move_card_action = ServerAction::<MoveCard>::new();

                                let board_signals = BoardSignals {
                                    list_order: RwSignal::new(list_order),
                                    list_cards: RwSignal::new(list_cards_map),
                                    card_signals: RwSignal::new(card_signals_map),
                                    done_list_ids: RwSignal::new(done_list_ids_set),
                                    drag_info: RwSignal::new(None),
                                    hover_list_id: RwSignal::new(None),
                                    before_card_id: RwSignal::new(None),
                                    search: RwSignal::new(String::new()),
                                    labels_expanded: RwSignal::new(false),
                                    move_card_action,
                                    board_id: RwSignal::new(data.board.id.clone()),
                                };

                                // Provide context for all child components
                                provide_context(board_signals);

                                // Still need StoredValue for lists (list mutations still use refetch)
                                let lists_sv = StoredValue::new(lists.clone());

                                view! {
                                    // Board header (breadcrumb + title + color chip + star + filter + labels)
                                    <BoardHeader
                                        board=board_clone
                                        search=board_signals.search
                                        labels_expanded=board_signals.labels_expanded
                                    />

                                    // Board canvas — horizontal scroll, gap 12px, padding 16px 20px
                                    <div class="lns-board-canvas">
                                        <Show
                                            when=move || lists_empty && !open_composer.get()
                                            fallback=move || {
                                                // Non-empty board: render KanbanList × N + AddListComposer
                                                let lists_for_render = lists_sv.get_value();
                                                let n = lists_for_render.len();

                                                view! {
                                                    {lists_for_render.into_iter().enumerate().map(|(idx, list)| {
                                                        // Compute neighbor positions for Move left/right (Pattern 4, D-15)
                                                        let can_move_left = idx > 0;
                                                        let can_move_right = idx < n - 1;

                                                        let lists_snap = lists_sv.get_value();

                                                        let left_neighbor_pos = if idx > 0 {
                                                            Some(lists_snap[idx - 1].position.clone())
                                                        } else {
                                                            None
                                                        };

                                                        let left_left_neighbor_pos = if idx >= 2 {
                                                            Some(lists_snap[idx - 2].position.clone())
                                                        } else {
                                                            None
                                                        };

                                                        let right_neighbor_pos = if idx < n - 1 {
                                                            Some(lists_snap[idx + 1].position.clone())
                                                        } else {
                                                            None
                                                        };

                                                        let right_right_neighbor_pos = if idx + 2 < n {
                                                            Some(lists_snap[idx + 2].position.clone())
                                                        } else {
                                                            None
                                                        };

                                                        view! {
                                                            <KanbanList
                                                                list=list
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

                                    // Cursor-following drag image (Trello-style). While a drag is in
                                    // progress, render a floating clone of the dragged card at the live
                                    // pointer position. current_x/current_y are updated on every
                                    // pointermove (kanban_list.rs); pointer-events:none keeps the clone
                                    // out of the drop hit-test. The source card stays dimmed in place.
                                    {
                                        let drag_info = board_signals.drag_info;
                                        let card_signals = board_signals.card_signals;
                                        move || {
                                            drag_info.get()
                                                .filter(|d| d.is_dragging)
                                                .and_then(|d| {
                                                    let title = card_signals.with(|cs| {
                                                        cs.get(&d.card_id).map(|sig| sig.get().title)
                                                    })?;
                                                    Some((d.current_x, d.current_y, title))
                                                })
                                                .map(|(x, y, title)| view! {
                                                    <div
                                                        class="lns-card lns-card-drag-image"
                                                        style=format!("left:{}px;top:{}px;", x, y)
                                                    >
                                                        <div class="lns-card-body">
                                                            <span class="lns-card-title">{title}</span>
                                                        </div>
                                                    </div>
                                                })
                                        }
                                    }
                                }.into_any()
                            }
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}
