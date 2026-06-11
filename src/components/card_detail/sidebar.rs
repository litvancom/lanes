//! SidebarColumn: "Add to card" + "Actions" groups + footer for the card-detail modal (Plan 06).
//!
//! Actions group:
//! - Move: popover with board selector + list selector; shows data-loss warning for cross-board
//!   (D-05); dispatches `MoveCardCrossBoard` or existing `MoveCard` for same-board
//! - Watch: toggles `WatchCard`, updates modal-scoped `watcher_count` + `is_watching`
//! - Archive: inline confirmation ("Archive this card?") — on confirm dispatches `ArchiveCard`,
//!   optimistically removes card from `BoardSignals.list_cards`, navigates back to board (D-15)
//!
//! Footer: `#LANES-C{card_num}` in JetBrains Mono + "Watching · {n} watchers" / "Watch · {n} watchers"

use leptos::prelude::*;
use crate::models::{UserSummary, MoveTargetBoard};
use crate::routes::board::BoardSignals;
use crate::api::card_detail_api::{WatchCard, ArchiveCard, MoveCardCrossBoard, list_move_targets};
use crate::api::card_api::MoveCard;
use crate::components::icon::Icon;
use crate::components::card_detail::pickers::{LabelPicker, DatePicker, PriorityPicker, MemberPicker};

/// Sidebar column: "Add to card" + "Actions" groups + footer.
///
/// Props:
/// - `board_id`: current board ID
/// - `card_id`: current card UUID
/// - `card_num`: for footer `#LANES-C{n}`
/// - `list_id`: current list ID (needed for Move same-board scope)
/// - `board_members`, `board_labels`: for Add-to-card pickers
/// - `watcher_count`, `is_watching`: modal-scoped reactive state for Watch
/// - `show_*` picker signals: one per picker in Add-to-card group
#[allow(clippy::too_many_arguments)]
#[component]
pub fn SidebarColumn(
    board_id: String,
    card_id: String,
    card_num: i64,
    list_id: String,
    board_members: Vec<UserSummary>,
    board_labels: Vec<crate::models::CardLabel>,
    watcher_count: RwSignal<i64>,
    is_watching: RwSignal<bool>,
    show_member_picker: RwSignal<bool>,
    show_label_picker: RwSignal<bool>,
    show_date_picker: RwSignal<bool>,
    show_priority_picker: RwSignal<bool>,
) -> impl IntoView {
    let board_id_sv = StoredValue::new(board_id.clone());
    let card_id_sv = StoredValue::new(card_id.clone());
    let list_id_sv = StoredValue::new(list_id.clone());
    let board_members_sv = StoredValue::new(board_members);
    let board_labels_sv = StoredValue::new(board_labels);

    // Board signals context for optimistic archive removal
    let board_signals: Option<BoardSignals> = use_context::<BoardSignals>();

    // ---- Move action state ----
    let show_move_popover = RwSignal::new(false);

    // Load move targets (user's boards + their lists) once, lazily when the popover opens.
    // Using Resource::new with a static key — the server fn is auth-gated and returns the
    // current user's accessible boards only (no archived boards/lists).
    let move_targets: Resource<Vec<MoveTargetBoard>> =
        Resource::new(|| (), |_| async { list_move_targets().await.unwrap_or_default() });

    let move_target_board_id = RwSignal::new(board_id.clone());
    let move_target_list_id = RwSignal::new(list_id.clone());
    // cross-board flag (target board != current board)
    // Stored as a move closure backed by RwSignals — captured by value so Fn is satisfied
    let current_board_id_sv = StoredValue::new(board_id.clone());
    let is_cross_board = move || move_target_board_id.get() != current_board_id_sv.get_value();

    let move_action = ServerAction::<MoveCard>::new();
    let move_cross_action = ServerAction::<MoveCardCrossBoard>::new();

    // ---- Watch action state ----
    let watch_action = ServerAction::<WatchCard>::new();
    // Tracks the watch state that was actually requested by the in-flight dispatch
    // (WR-01). On success we set is_watching to this authoritative value rather than
    // blindly negating the current value, which desyncs on rapid clicks / in-flight
    // toggles since the server returns only the watcher count.
    let requested_watch = RwSignal::new(is_watching.get_untracked());

    // When watch action succeeds, update modal-scoped watcher signals
    Effect::new(move |_| {
        if let Some(Ok(new_count)) = watch_action.value().get() {
            watcher_count.set(new_count);
            // Set to the requested value, not a blind negation (WR-01)
            is_watching.set(requested_watch.get_untracked());
        }
    });

    // ---- Archive action state ----
    let show_archive_confirm = RwSignal::new(false);
    let archive_action = ServerAction::<ArchiveCard>::new();

    // When archive succeeds: optimistically remove from board surface + navigate back
    Effect::new(move |_| {
        if let Some(Ok(())) = archive_action.value().get() {
            let card_id = card_id_sv.get_value();
            let board_id = board_id_sv.get_value();
            // Optimistic removal from BoardSignals.list_cards (D-15)
            if let Some(bs) = board_signals {
                bs.list_cards.update(|lc| {
                    for cards in lc.values_mut() {
                        cards.retain(|cid| cid != &card_id);
                    }
                });
            }
            // Navigate back to board
            #[cfg(target_arch = "wasm32")]
            {
                use leptos_router::hooks::use_navigate;
                use leptos_router::NavigateOptions;
                let navigate = use_navigate();
                let path = format!("/board/{}", board_id);
                navigate(&path, NavigateOptions { replace: true, ..Default::default() });
            }
        }
    });

    use fractional_index::FractionalIndex;
    let default_pos = FractionalIndex::default().to_string();

    view! {
        <div class="lns-modal-sidebar">
            // ── Add to card group ─────────────────────────────────────────
            <div class="group">
                <div style="font-size: 11px; font-weight: 600; color: var(--text-muted); letter-spacing: 0.04em; text-transform: uppercase; margin: 0 2px 2px">
                    "Add to card"
                </div>
                // Members button + picker
                <div style="position: relative">
                    <button
                        class="lns-btn"
                        on:click=move |_| show_member_picker.update(|v| *v = !*v)
                    >
                        <Icon name="users"/>
                        " Members"
                    </button>
                    <MemberPicker
                        board_id=board_id_sv.get_value()
                        card_id=card_id_sv.get_value()
                        board_members=board_members_sv.get_value()
                        card_signal_key=card_id_sv.get_value()
                        show=show_member_picker
                    />
                </div>
                // Labels button + picker
                <div style="position: relative">
                    <button
                        class="lns-btn"
                        on:click=move |_| show_label_picker.update(|v| *v = !*v)
                    >
                        <Icon name="tag"/>
                        " Labels"
                    </button>
                    <LabelPicker
                        board_id=board_id_sv.get_value()
                        card_id=card_id_sv.get_value()
                        board_labels=board_labels_sv.get_value()
                        card_signal_key=card_id_sv.get_value()
                        show=show_label_picker
                    />
                </div>
                // Checklist button (scrolls to section)
                <button
                    class="lns-btn"
                    on:click=move |_| {
                        #[cfg(target_arch = "wasm32")]
                        {
                            use wasm_bindgen::JsCast;
                            if let Some(window) = leptos::web_sys::window() {
                                if let Some(doc) = window.document() {
                                    if let Some(el) = doc.get_element_by_id("card-detail-checklist-section") {
                                        if let Ok(el) = el.dyn_into::<leptos::web_sys::HtmlElement>() {
                                            el.scroll_into_view();
                                        }
                                    }
                                }
                            }
                        }
                    }
                >
                    <Icon name="check"/>
                    " Checklist"
                </button>
                // Dates button + picker
                <div style="position: relative">
                    <button
                        class="lns-btn"
                        on:click=move |_| show_date_picker.update(|v| *v = !*v)
                    >
                        <Icon name="calendar"/>
                        " Dates"
                    </button>
                    <DatePicker
                        board_id=board_id_sv.get_value()
                        card_id=card_id_sv.get_value()
                        card_signal_key=card_id_sv.get_value()
                        show=show_date_picker
                    />
                </div>
                // Attachment button — triggers the hidden file input (DETAIL-08)
                <button
                    class="lns-btn"
                    on:click=move |_| {
                        #[cfg(target_arch = "wasm32")]
                        {
                            use wasm_bindgen::JsCast;
                            // Per-card input id (WR-07) — must match AttachmentsSection's derived id
                            let input_id = format!("card-attachment-input-{}", card_id_sv.get_value());
                            if let Some(window) = leptos::web_sys::window() {
                                if let Some(doc) = window.document() {
                                    if let Some(el) = doc.get_element_by_id(&input_id) {
                                        if let Ok(input) = el.dyn_into::<leptos::web_sys::HtmlElement>() {
                                            let _ = input.click();
                                        }
                                    }
                                }
                            }
                        }
                    }
                >
                    <Icon name="paperclip"/>
                    " Attachment"
                </button>
                // Priority button + picker
                <div style="position: relative">
                    <button
                        class="lns-btn"
                        on:click=move |_| show_priority_picker.update(|v| *v = !*v)
                    >
                        <Icon name="flag"/>
                        " Priority"
                    </button>
                    <PriorityPicker
                        board_id=board_id_sv.get_value()
                        card_id=card_id_sv.get_value()
                        card_signal_key=card_id_sv.get_value()
                        show=show_priority_picker
                    />
                </div>
            </div>

            // ── Actions group ────────────────────────────────────────────
            <div class="group">
                <div style="font-size: 11px; font-weight: 600; color: var(--text-muted); letter-spacing: 0.04em; text-transform: uppercase; margin: 0 2px 2px">
                    "Actions"
                </div>

                // Move button + popover
                <div style="position: relative">
                    <button
                        class="lns-btn"
                        on:click=move |_| show_move_popover.update(|v| *v = !*v)
                    >
                        <Icon name="moveTo"/>
                        " Move"
                    </button>
                    <Show when=move || show_move_popover.get()>
                        <div class="lns-action-popover">
                            // Board selector
                            <div class="lns-action-popover-row">
                                <label class="lns-action-popover-label">"Board"</label>
                                <Suspense fallback=move || view! { <select class="lns-action-popover-input" disabled=true><option>"Loading…"</option></select> }>
                                    {move || {
                                        let boards = move_targets.get().unwrap_or_default();
                                        let current_sel = move_target_board_id.get();
                                        view! {
                                            <select
                                                class="lns-action-popover-input"
                                                prop:value=current_sel.clone()
                                                on:change=move |ev| {
                                                    let new_board = event_target_value(&ev);
                                                    // When board changes, reset list to first list of that board
                                                    let boards_snap = move_targets.get().unwrap_or_default();
                                                    let first_list = boards_snap
                                                        .iter()
                                                        .find(|b| b.id == new_board)
                                                        .and_then(|b| b.lists.first())
                                                        .map(|l| l.id.clone())
                                                        .unwrap_or_default();
                                                    move_target_board_id.set(new_board);
                                                    move_target_list_id.set(first_list);
                                                }
                                            >
                                                {boards.into_iter().map(|b| {
                                                    let selected = b.id == current_sel;
                                                    view! {
                                                        <option value=b.id.clone() selected=selected>{b.name}</option>
                                                    }
                                                }).collect::<Vec<_>>()}
                                            </select>
                                        }
                                    }}
                                </Suspense>
                            </div>
                            // List selector — options depend on the selected board
                            <div class="lns-action-popover-row">
                                <label class="lns-action-popover-label">"List"</label>
                                <Suspense fallback=move || view! { <select class="lns-action-popover-input" disabled=true><option>"Loading…"</option></select> }>
                                    {move || {
                                        let boards = move_targets.get().unwrap_or_default();
                                        let sel_board = move_target_board_id.get();
                                        let sel_list = move_target_list_id.get();
                                        let lists = boards
                                            .iter()
                                            .find(|b| b.id == sel_board)
                                            .map(|b| b.lists.clone())
                                            .unwrap_or_default();
                                        view! {
                                            <select
                                                class="lns-action-popover-input"
                                                prop:value=sel_list.clone()
                                                on:change=move |ev| {
                                                    move_target_list_id.set(event_target_value(&ev));
                                                }
                                            >
                                                {lists.into_iter().map(|l| {
                                                    let selected = l.id == sel_list;
                                                    view! {
                                                        <option value=l.id.clone() selected=selected>{l.name}</option>
                                                    }
                                                }).collect::<Vec<_>>()}
                                            </select>
                                        }
                                    }}
                                </Suspense>
                            </div>
                            // Cross-board data-loss warning (Copywriting Contract, D-05)
                            <Show when=is_cross_board>
                                <div class="lns-action-warning">
                                    "Labels and non-member assignees will be removed."
                                </div>
                            </Show>
                            <div style="display: flex; gap: 6px; margin-top: 6px">
                                <button
                                    class="lns-btn lns-btn--primary lns-btn--sm"
                                    on:click={
                                        let default_pos_clone = default_pos.clone();
                                        move |_| {
                                            let tboard = move_target_board_id.get_untracked();
                                            let tlist = move_target_list_id.get_untracked();
                                            let cid = card_id_sv.get_value();
                                            let bid = board_id_sv.get_value();
                                            let pos = default_pos_clone.clone();
                                            if tboard == bid {
                                                // Same-board: use Phase 4 move_card
                                                move_action.dispatch(MoveCard {
                                                    board_id: bid,
                                                    card_id: cid,
                                                    to_list_id: tlist,
                                                    new_position: pos,
                                                });
                                            } else {
                                                // Cross-board: use move_card_cross_board
                                                move_cross_action.dispatch(MoveCardCrossBoard {
                                                    from_board_id: bid,
                                                    card_id: cid,
                                                    to_board_id: tboard,
                                                    to_list_id: tlist,
                                                    new_position: pos,
                                                });
                                            }
                                            show_move_popover.set(false);
                                        }
                                    }
                                >
                                    "Move"
                                </button>
                                <button
                                    class="lns-btn lns-btn--ghost lns-btn--sm"
                                    on:click=move |_| show_move_popover.set(false)
                                >
                                    "Cancel"
                                </button>
                            </div>
                        </div>
                    </Show>
                </div>

                // Watch button
                <button
                    class="lns-btn"
                    on:click=move |_| {
                        let bid = board_id_sv.get_value();
                        let cid = card_id_sv.get_value();
                        let currently_watching = is_watching.get_untracked();
                        let want = !currently_watching;
                        requested_watch.set(want);
                        watch_action.dispatch(WatchCard {
                            board_id: bid,
                            card_id: cid,
                            watch: want,
                        });
                    }
                >
                    <Icon name="eye"/>
                    {move || if is_watching.get() { " Watching" } else { " Watch" }}
                </button>

                // Archive button + inline confirmation (Copywriting Contract)
                <div>
                    <button
                        class="lns-btn"
                        on:click=move |_| show_archive_confirm.update(|v| *v = !*v)
                    >
                        <Icon name="archive"/>
                        " Archive"
                    </button>
                    <Show when=move || show_archive_confirm.get()>
                        <div class="lns-archive-confirm">
                            <div style="font-size: 12px; color: var(--text-secondary); margin-bottom: 6px">
                                "Archive this card?"
                            </div>
                            <div style="display: flex; gap: 6px">
                                <button
                                    class="lns-btn lns-btn--primary lns-btn--sm lns-btn--archive-confirm"
                                    on:click=move |_| {
                                        let bid = board_id_sv.get_value();
                                        let cid = card_id_sv.get_value();
                                        archive_action.dispatch(ArchiveCard {
                                            board_id: bid,
                                            card_id: cid,
                                        });
                                        show_archive_confirm.set(false);
                                    }
                                >
                                    "Archive"
                                </button>
                                <button
                                    class="lns-btn lns-btn--ghost lns-btn--sm"
                                    on:click=move |_| show_archive_confirm.set(false)
                                >
                                    "Cancel"
                                </button>
                            </div>
                        </div>
                    </Show>
                </div>
            </div>

            // ── Footer: #LANES-Cnn + watcher count ───────────────────────
            <div style="font-size: 11px; color: var(--text-faint); padding: 0 2px">
                <div class="lns-mono">
                    {format!("#LANES-C{}", card_num)}
                </div>
                <div style="margin-top: 2px">
                    {move || {
                        let n = watcher_count.get();
                        if is_watching.get() {
                            format!("Watching · {} watchers", n)
                        } else {
                            format!("Watch · {} watchers", n)
                        }
                    }}
                </div>
            </div>
        </div>
    }
}
