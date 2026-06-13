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
use leptos_router::components::Outlet;
use std::collections::{HashMap, HashSet};
use crate::models::Card;
use crate::api::board_api::{get_board, TouchLastViewed};
use crate::api::auth_api::get_current_user;
use crate::api::notification_api::get_unread_count;
use crate::api::list_api::{CreateList, RenameList, ReorderList};
use crate::api::card_api::MoveCard;
use crate::components::board_header::BoardHeader;
use crate::components::kanban_list::{KanbanList, AddListComposer, EmptyBoardCard};
use crate::components::create_board_modal::CreateBoardModal;
use crate::components::reconnect_toast::ReconnectToast;

/// Wrapper around a WebSocket send closure that is `Clone + Send + Sync`.
///
/// Uses `Arc` so it can be cloned out of `StoredValue::get_value()`.
/// Safety contract: this is only called from WASM microtask context (single-threaded).
/// On SSR (x86 multi-threaded) the Option is always None so the fn is never invoked.
#[derive(Clone)]
pub struct WsSendFn(pub std::sync::Arc<WsSendFnInner>);

/// Inner wrapper that satisfies Send + Sync via unsafe impl.
/// SAFETY: WASM is single-threaded; on SSR this is never constructed or called.
pub struct WsSendFnInner(pub Box<dyn Fn(String) + 'static>);
unsafe impl Send for WsSendFnInner {}
unsafe impl Sync for WsSendFnInner {}

impl WsSendFn {
    pub fn new(f: impl Fn(String) + 'static) -> Self {
        Self(std::sync::Arc::new(WsSendFnInner(Box::new(f))))
    }

    pub fn call(&self, msg: String) {
        (self.0.0)(msg);
    }
}

/// Client-side display struct for a presence viewer (RT-03).
/// Defined here so BoardSignals can hold a Vec of these without importing presence_registry
/// (which is ssr-only). This is a small copy of the display fields from PresenceState.
#[derive(Clone, Debug, PartialEq)]
pub struct PresenceViewer {
    pub user_id: String,
    pub display_name: String,
    pub avatar_color: String,
}

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
    /// Reactive board name — seeded from SSR data; updated live by BoardRenamed events.
    pub board_name: RwSignal<String>,
    // ── Realtime (Phase 6) ──────────────────────────────────────────────────
    /// The client_id assigned by the server in the Connected handshake.
    /// Stored here so every mutation dispatch can include it (D-05/Flag 2).
    /// None = WS not yet connected or reconnecting.
    pub own_client_id: RwSignal<Option<String>>,
    /// Last board_seq value received from the server.
    /// Used for gap detection: if next event's board_seq > last_seen_seq + 1, trigger Refresh.
    pub last_seen_seq: RwSignal<u64>,
    /// Card currently highlighted due to a remote move (D-04/D-05).
    /// None = no highlight active. Set by apply_board_event; cleared after ~1.5s.
    pub highlight_card_id: RwSignal<Option<String>>,
    /// Cards currently playing the fade-collapse animation (D-06: remote archive).
    /// Inserted by apply_board_event on CardArchived; removed after 350ms once CSS animation ends.
    pub fading_card_ids: RwSignal<HashSet<String>>,
    /// Set by apply_board_event when a remote CardArchived event arrives for a card that is
    /// currently open in the detail modal (D-09). The modal watches this signal and auto-closes
    /// with a "archived by another user" notice after 2500ms. Cleared by the modal on close.
    pub remote_archived_card_id: RwSignal<Option<String>>,
    // ── RT-02 Reconnect state (Phase 6) ────────────────────────────────────
    /// Number of consecutive failed reconnect attempts (D-01/RT-02).
    /// 0 = connected or no failure yet. Set by the reconnect loop; read by ReconnectToast.
    /// The toast renders only when reconnect_attempts >= 2 (silent on first transient drop).
    pub reconnect_attempts: RwSignal<u32>,
    /// True when the WebSocket is successfully connected (received Connected handshake).
    /// False while connecting, disconnected, or reconnecting.
    pub ws_connected: RwSignal<bool>,
    // ── Presence (06-04) ───────────────────────────────────────────────────
    /// Current board viewers, excluding the current user (SC5: you don't see yourself).
    /// Patched by WsEnvelope::Presence events: ViewersSnapshot replaces, ViewerJoined appends,
    /// ViewerLeft removes.
    pub viewers: RwSignal<Vec<PresenceViewer>>,
    /// Maps card_id → Vec of display_names of users currently editing that card (D-10).
    /// Patched by EditingCard presence events.
    pub editing_card_ids: RwSignal<HashMap<String, Vec<String>>>,
    /// Maps card_id → Vec of display_names of users currently typing in that card (D-10).
    /// Patched by Typing presence events.
    pub typing_card_ids: RwSignal<HashMap<String, Vec<String>>>,
    /// Client→server WS send function, set by spawn_ws_task once connected.
    /// Used by UI components to emit heartbeat/editing/typing messages without holding
    /// a direct reference to the WebSocket (which is WASM-only).
    ///
    /// The inner function is wrapped in `WsSendFn` which is `Send + Sync` via a
    /// safety contract: this is only called from WASM microtask context (single-threaded)
    /// and never crosses thread boundaries at runtime.
    pub ws_send: StoredValue<Option<WsSendFn>>,
    /// The current user's own user_id (from session), used to exclude self from the viewer
    /// stack (SC5: you don't see yourself). Set once during BoardPage mount via a server fn.
    /// None on SSR (not needed server-side).
    pub own_user_id: RwSignal<Option<String>>,
    // ── RT-04 Notification badge (06-05) ──────────────────────────────────────
    /// Live unread notification count — seeds from get_unread_count() and is patched live by
    /// NotifEvent::UnreadCountUpdated arriving on the per-user WS channel.
    /// Drives the sidebar inbox badge (UI-SPEC §7).
    pub unread_count: RwSignal<i64>,
    /// True for ~200ms when unread_count increments — triggers the CSS pulse animation.
    /// Set by the WsEnvelope::User dispatch arm; cleared by a TimeoutFuture.
    pub badge_pulse: RwSignal<bool>,
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
                                    board_name: RwSignal::new(data.board.name.clone()),
                                    // Phase 6 realtime fields
                                    own_client_id: RwSignal::new(None),
                                    last_seen_seq: RwSignal::new(data.board_seq),
                                    highlight_card_id: RwSignal::new(None),
                                    fading_card_ids: RwSignal::new(HashSet::new()),
                                    remote_archived_card_id: RwSignal::new(None),
                                    // RT-02 reconnect state
                                    reconnect_attempts: RwSignal::new(0),
                                    ws_connected: RwSignal::new(false),
                                    // Presence (06-04)
                                    viewers: RwSignal::new(Vec::new()),
                                    editing_card_ids: RwSignal::new(HashMap::new()),
                                    typing_card_ids: RwSignal::new(HashMap::new()),
                                    ws_send: StoredValue::new(None),
                                    own_user_id: RwSignal::new(None),
                                    // RT-04 notification badge (06-05)
                                    unread_count: RwSignal::new(0),
                                    badge_pulse: RwSignal::new(false),
                                };

                                // Provide context for all child components
                                provide_context(board_signals);

                                // ── Own user_id for presence self-exclusion (SC5) ──────────
                                // Fetch the current user's ID client-side via server fn so the
                                // presence stack can exclude the current viewer (SC5 rule).
                                // Runs in an Effect so it never blocks SSR rendering (Pitfall 6).
                                {
                                    let own_uid_sig = board_signals.own_user_id;
                                    Effect::new(move |_| {
                                        #[cfg(target_arch = "wasm32")]
                                        wasm_bindgen_futures::spawn_local(async move {
                                            if let Ok(Some(user)) = get_current_user().await {
                                                own_uid_sig.set(Some(user.id));
                                            }
                                        });
                                        let _ = own_uid_sig; // suppress unused warning in SSR
                                    });
                                }

                                // ── RT-04 Unread count seed (06-05) ────────────────────────
                                // Fetch initial unread notification count client-side so the
                                // sidebar badge shows the correct number on board load.
                                // Live updates arrive over the per-user WS channel.
                                {
                                    let unread_sig = board_signals.unread_count;
                                    Effect::new(move |_| {
                                        #[cfg(target_arch = "wasm32")]
                                        wasm_bindgen_futures::spawn_local(async move {
                                            if let Ok(count) = get_unread_count().await {
                                                unread_sig.set(count);
                                            }
                                        });
                                        let _ = unread_sig; // suppress unused warning in SSR
                                    });
                                }

                                // ── Realtime WS task (Phase 6) ──────────────────────────────
                                // Spawn the WS client task on mount (WASM only).
                                // on_cleanup drops the WsHandle which signals abort.
                                {
                                    use crate::state::ws_client::spawn_ws_task;
                                    let ws_board_id = data.board.id.clone();
                                    let ws_handle = StoredValue::new(
                                        Some(spawn_ws_task(ws_board_id, board_signals))
                                    );
                                    on_cleanup(move || {
                                        // Drop the WsHandle to signal abort
                                        ws_handle.update_value(|h| { h.take(); });
                                    });
                                }

                                // Still need StoredValue for lists (list mutations still use refetch)
                                let lists_sv = StoredValue::new(lists.clone());

                                view! {
                                    // Board header (breadcrumb + title + color chip + star + filter + labels)
                                    <BoardHeader
                                        board=board_clone
                                        search=board_signals.search
                                        labels_expanded=board_signals.labels_expanded
                                        is_owner=data.viewer_is_owner
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
                                    // Card detail modal renders here when /board/:id/card/:card_num is active
                                    <Outlet/>

                                    // Reconnecting toast — shown after 2+ failed reconnect attempts (D-01, RT-02)
                                    <ReconnectToast/>
                                }.into_any()
                            }
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}
