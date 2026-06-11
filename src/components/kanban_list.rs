use leptos::prelude::*;
use crate::models::List;
use crate::api::list_api::{CreateList, RenameList, ReorderList};
use crate::api::card_api::{CreateCard, MoveCard};
use crate::routes::board::{BoardSignals, DragInfo};
use crate::components::kanban_card::KanbanCard;

// ---------------------------------------------------------------------------
// KanbanList — full list component with rename, signal-based cards, AddCardComposer
// ---------------------------------------------------------------------------

/// A kanban list with inline rename, signal-based card rendering, and AddCardComposer.
///
/// Reads `BoardSignals` from context to get card signals and filter state.
/// List mutations (rename/reorder) still dispatch server actions and trigger refetch.
/// Card creation is optimistic — no refetch.
#[component]
pub fn KanbanList(
    list: List,
    can_move_left: bool,
    can_move_right: bool,
    /// Position of the list immediately to the left (for Move left computation)
    left_neighbor_pos: Option<String>,
    /// Position two slots to the left (for Move left midpoint when not at index 0)
    left_left_neighbor_pos: Option<String>,
    /// Position of the list immediately to the right (for Move right computation)
    right_neighbor_pos: Option<String>,
    /// Position two slots to the right (for Move right midpoint when not at last slot)
    right_right_neighbor_pos: Option<String>,
    rename_action: ServerAction<RenameList>,
    reorder_action: ServerAction<ReorderList>,
) -> impl IntoView {
    // Read BoardSignals from context (provided by board.rs)
    let board_signals = use_context::<BoardSignals>().expect("BoardSignals context missing");
    let list_cards = board_signals.list_cards;
    let card_signals = board_signals.card_signals;
    let labels_expanded = board_signals.labels_expanded;
    let search = board_signals.search;
    let drag_info = board_signals.drag_info;
    let hover_list_id = board_signals.hover_list_id;
    let before_card_id = board_signals.before_card_id;
    let done_list_ids = board_signals.done_list_ids;
    let move_card_action = board_signals.move_card_action;
    let board_id_sig = board_signals.board_id;

    // --- Drag error banner (non-blocking rollback message) ---
    let drag_error: RwSignal<Option<String>> = RwSignal::new(None);

    // --- Drag list ID (captured for drag-over class reactive check) ---
    let list_id_for_drag_class = list.id.clone();

    // --- Document-level pointermove: update position, threshold check, hit-test ---
    use leptos_use::use_event_listener;
    use leptos_use::use_document;

    use_event_listener(use_document(), leptos::ev::pointermove, move |ev: leptos::ev::PointerEvent| {
        let has_drag = drag_info.with(|d| d.is_some());
        if !has_drag { return; }

        let x = ev.client_x() as f64;
        let y = ev.client_y() as f64;

        drag_info.update(|d| {
            if let Some(info) = d.as_mut() {
                // Enter drag state once threshold exceeded (5px per UI-SPEC)
                if !info.is_dragging {
                    let dx = x - info.start_x;
                    let dy = y - info.start_y;
                    if (dx * dx + dy * dy).sqrt() > 5.0 {
                        info.is_dragging = true;
                    }
                }
                info.current_x = x;
                info.current_y = y;
            }
        });

        // Hit-test: find which list (and before-card) is under the pointer (client-only)
        #[cfg(target_arch = "wasm32")]
        {
            update_hover_target(x, y, hover_list_id, before_card_id);
        }
    });

    // --- Document-level pointerup: commit drop if dragging ---
    use_event_listener(use_document(), leptos::ev::pointerup, move |_ev: leptos::ev::PointerEvent| {
        let current = drag_info.get_untracked();
        if let Some(info) = current {
            if info.is_dragging {
                commit_drop(info, board_signals, drag_error);
            }
        }
        drag_info.set(None);
        hover_list_id.set(None);
        before_card_id.set(None);
    });

    // --- Document-level pointercancel: snap back (UI-SPEC line 387) ---
    use_event_listener(use_document(), leptos::ev::pointercancel, move |_ev: leptos::ev::PointerEvent| {
        drag_info.set(None);
        hover_list_id.set(None);
        before_card_id.set(None);
    });

    // Watch move_card_action for errors → rollback already done in commit_drop via Effect
    // The rollback Effect is per-drop (created inside commit_drop). Nothing needed here.

    // --- Inline rename state ---
    let editing = RwSignal::new(false);
    let title_input_ref = NodeRef::<leptos::html::Input>::new();

    // Auto-focus when entering edit mode (Pattern 3)
    Effect::new(move |_| {
        if editing.get() {
            if let Some(input) = title_input_ref.get() {
                let _ = input.focus();
            }
        }
    });

    // --- Overflow menu state ---
    let menu_open = RwSignal::new(false);

    // Store mutable state as StoredValues so closures can be Fn
    let list_id_sv = StoredValue::new(list.id.clone());
    let list_name_sv = StoredValue::new(list.name.clone());

    // Stored neighbor positions for reorder (avoid move-out-of-Option issues)
    let left_nb = StoredValue::new(left_neighbor_pos);
    let left_left_nb = StoredValue::new(left_left_neighbor_pos);
    let right_nb = StoredValue::new(right_neighbor_pos);
    let right_right_nb = StoredValue::new(right_right_neighbor_pos);

    // --- Commit rename — Fn compatible via StoredValue ---
    let commit_rename = move || {
        if let Some(input) = title_input_ref.get() {
            let new_name = input.value();
            let trimmed = new_name.trim().to_string();
            let orig = list_name_sv.get_value();
            if !trimmed.is_empty() && trimmed != orig {
                rename_action.dispatch(RenameList {
                    list_id: list_id_sv.get_value(),
                    name: trimmed,
                });
            }
        }
        editing.set(false);
    };

    // --- Move left handler ---
    let on_move_left = move |_: leptos::ev::MouseEvent| {
        use fractional_index::FractionalIndex;
        menu_open.set(false);

        let right_str = match left_nb.get_value() {
            Some(s) => s,
            None => return,
        };

        let new_pos = match left_left_nb.get_value() {
            Some(left_str) => {
                match (FractionalIndex::from_string(&left_str), FractionalIndex::from_string(&right_str)) {
                    (Ok(lfi), Ok(rfi)) => FractionalIndex::new_between(&lfi, &rfi).map(|fi| fi.to_string()),
                    _ => None,
                }
            }
            None => {
                FractionalIndex::from_string(&right_str).ok()
                    .map(|fi| FractionalIndex::new_before(&fi).to_string())
            }
        };

        if let Some(new_position) = new_pos {
            reorder_action.dispatch(ReorderList {
                list_id: list_id_sv.get_value(),
                new_position,
            });
        }
    };

    // --- Move right handler ---
    let on_move_right = move |_: leptos::ev::MouseEvent| {
        use fractional_index::FractionalIndex;
        menu_open.set(false);

        let left_str = match right_nb.get_value() {
            Some(s) => s,
            None => return,
        };

        let new_pos = match right_right_nb.get_value() {
            Some(right_str) => {
                match (FractionalIndex::from_string(&left_str), FractionalIndex::from_string(&right_str)) {
                    (Ok(lfi), Ok(rfi)) => FractionalIndex::new_between(&lfi, &rfi).map(|fi| fi.to_string()),
                    _ => None,
                }
            }
            None => {
                FractionalIndex::from_string(&left_str).ok()
                    .map(|fi| FractionalIndex::new_after(&fi).to_string())
            }
        };

        if let Some(new_position) = new_pos {
            reorder_action.dispatch(ReorderList {
                list_id: list_id_sv.get_value(),
                new_position,
            });
        }
    };

    // Capture list_id as a plain string for <For> closure
    let list_id_for_render = list.id.clone();
    let list_name_display = list.name.clone();
    let list_name_input_default = list.name.clone();

    // Reactive card count for the header pill (reads from list_cards signal)
    let list_id_for_count = list.id.clone();
    let card_count = move || {
        list_cards.with(|m| m.get(&list_id_for_count).map(|v| v.len()).unwrap_or(0))
    };

    view! {
        <div
            class="lns-list"
            class:drag-over={
                let lid = list_id_for_drag_class.clone();
                move || hover_list_id.get().map_or(false, |id| id == lid)
            }
            data-list-id=list_id_for_drag_class.clone()
        >
            // ── Drag rollback error banner (non-blocking) ────────────────
            <Show when=move || drag_error.get().is_some()>
                <div class="lns-error-inline">
                    {move || drag_error.get().unwrap_or_default()}
                </div>
            </Show>

            // ── List Header ──────────────────────────────────────────────
            <div class="lns-list-header">
                <Show
                    when=move || editing.get()
                    fallback=move || view! {
                        <span
                            class="lns-list-title"
                            on:click=move |_| editing.set(true)
                        >
                            {list_name_display.clone()}
                        </span>
                    }
                >
                    <input
                        node_ref=title_input_ref
                        class="lns-list-title-input"
                        type="text"
                        prop:value=list_name_input_default.clone()
                        on:keydown={
                            let commit = commit_rename.clone();
                            move |e: leptos::ev::KeyboardEvent| {
                                match e.key().as_str() {
                                    "Enter" => commit(),
                                    "Escape" => editing.set(false),
                                    _ => {}
                                }
                            }
                        }
                        on:blur={
                            let commit = commit_rename.clone();
                            move |_| commit()
                        }
                    />
                </Show>

                // Count pill (reactive from BoardSignals)
                <span class="lns-list-count">{card_count}</span>

                // Overflow dots button
                <button
                    type="button"
                    class="lns-list-menu-btn"
                    aria-label="List options"
                    on:click=move |_| menu_open.update(|v| *v = !*v)
                >
                    <crate::components::icon::Icon name="dots"/>
                </button>

                // Overflow menu — Move left / Move right
                <Show when=move || menu_open.get()>
                    <div class="lns-list-menu">
                        <button
                            type="button"
                            class="lns-list-menu-item"
                            class:lns-list-menu-item--disabled=!can_move_left
                            disabled=!can_move_left
                            on:click=on_move_left
                        >
                            "Move left"
                        </button>
                        <button
                            type="button"
                            class="lns-list-menu-item"
                            class:lns-list-menu-item--disabled=!can_move_right
                            disabled=!can_move_right
                            on:click=on_move_right
                        >
                            "Move right"
                        </button>
                    </div>
                </Show>
            </div>

            // ── Cards area ───────────────────────────────────────────────
            <div class="lns-list-cards">
                // Signal-based <For> with reactive filter (Pitfall 2 + Pitfall 4 + Pitfall 6)
                <For
                    each={
                        let list_id_c = list_id_for_render.clone();
                        move || {
                            // Pitfall 6: filter must be inside reactive closure reading search.get()
                            let q = search.get().to_lowercase();
                            // Pitfall 4: use .with() to avoid cloning the entire HashMap
                            let ids = list_cards.with(|m| {
                                m.get(&list_id_c).cloned().unwrap_or_default()
                            });
                            ids.into_iter()
                                .filter(|id| {
                                    if q.is_empty() { return true; }
                                    card_signals.with(|cs| {
                                        cs.get(id).map_or(false, |sig| {
                                            let c = sig.get();
                                            c.title.to_lowercase().contains(&q)
                                                || c.labels.iter().any(|l| l.name.to_lowercase().contains(&q))
                                        })
                                    })
                                })
                                // Pitfall 4: use .with() to get the signal without cloning the map
                                .filter_map(|id| card_signals.with(|cs| cs.get(&id).copied()))
                                .collect::<Vec<_>>()
                        }
                    }
                    // Pitfall 2: key by untracked ID to avoid creating reactive subscriptions in key fn
                    key=|sig| sig.get_untracked().id.clone()
                    let(card_sig)
                >
                    <KanbanCard
                        card=card_sig
                        labels_expanded=labels_expanded
                        list_id=list_id_for_render.clone()
                        drag_info=drag_info
                    />
                </For>
            </div>

            // ── Footer: "Add a card" / AddCardComposer ────────────────────
            // Hidden entirely when filter is active (UI-SPEC line 308, CARD-05 cross-cut)
            <Show when=move || search.get().trim().is_empty()>
                <div class="lns-list-footer">
                    <AddCardComposer
                        list_id=list.id.clone()
                        board_id=list.board_id.clone()
                        board_signals=board_signals
                    />
                </div>
            </Show>
        </div>
    }
}

// ---------------------------------------------------------------------------
// Drag helper: Update hover_list_id based on pointer position (client-only)
// ---------------------------------------------------------------------------

/// Walk document.elementsFromPoint to find which list and before-card the pointer is over.
/// Sets hover_list_id and before_card_id based on what's under the pointer.
/// WASM-only — no-op on server.
#[cfg(target_arch = "wasm32")]
fn update_hover_target(
    x: f64,
    y: f64,
    hover_list_id: leptos::prelude::RwSignal<Option<String>>,
    before_card_id: leptos::prelude::RwSignal<Option<String>>,
) {
    use wasm_bindgen::JsCast;
    let Some(window) = leptos::web_sys::window() else { return; };
    let Some(doc) = window.document() else { return; };
    let elements = doc.elements_from_point(x as f32, y as f32);
    let mut found_list: Option<String> = None;
    let mut found_card: Option<String> = None;
    let len = elements.length();
    for i in 0..len {
        let js_val = elements.get(i);
        let Some(el) = js_val.dyn_into::<leptos::web_sys::Element>().ok()
        else { continue; };

        if found_card.is_none() {
            if let Some(card_id) = el.get_attribute("data-card-id") {
                // We're over a card; the dragged card will be inserted before this card
                found_card = Some(card_id);
                // Its data-list-id is the target list
                if let Some(lid) = el.get_attribute("data-list-id") {
                    if found_list.is_none() {
                        found_list = Some(lid);
                    }
                }
            }
        }

        if found_list.is_none() {
            if let Some(lid) = el.get_attribute("data-list-id") {
                found_list = Some(lid);
            }
        }

        // Stop once we have both pieces of info
        if found_list.is_some() && found_card.is_some() {
            break;
        }
        // If we have a list but no card, we've found the list backdrop
        if found_list.is_some() && found_card.is_none() {
            // No card above list element in the stack — append to list
            break;
        }
    }
    hover_list_id.set(found_list);
    before_card_id.set(found_card);
}

// ---------------------------------------------------------------------------
// Drag helper: Compute card drop position using fractional indexing (CARD-04)
// ---------------------------------------------------------------------------

/// Compute a new fractional position for dropping a card.
///
/// `cards_in_list`: ordered card IDs in the target list (EXCLUDING the dragged card — Pitfall 3).
/// `before_card_id`: the card the dragged card will be inserted before (None = append to list).
///
/// Returns `None` only if the fractional index key space is exhausted between two adjacent keys
/// (extremely rare in practice).
fn compute_card_drop_position(
    cards_in_list: &[String],
    card_signals: &std::collections::HashMap<String, leptos::prelude::RwSignal<crate::models::Card>>,
    before_card_id: Option<&str>,
) -> Option<String> {
    use fractional_index::FractionalIndex;

    if before_card_id.is_none() {
        // Append to list
        if let Some(last_id) = cards_in_list.last() {
            let last_pos = card_signals.get(last_id)?.get_untracked().position;
            FractionalIndex::from_string(&last_pos).ok()
                .map(|fi| FractionalIndex::new_after(&fi).to_string())
        } else {
            // Empty list
            Some(FractionalIndex::default().to_string())
        }
    } else {
        let before_id = before_card_id.unwrap();
        let before_idx = cards_in_list.iter().position(|id| id == before_id)?;
        let before_pos = card_signals.get(before_id)?.get_untracked().position;
        let before_fi = FractionalIndex::from_string(&before_pos).ok()?;

        if before_idx == 0 {
            // Prepend — new_before
            Some(FractionalIndex::new_before(&before_fi).to_string())
        } else {
            // Insert between prev and before
            let prev_id = &cards_in_list[before_idx - 1];
            let prev_pos = card_signals.get(prev_id)?.get_untracked().position;
            let prev_fi = FractionalIndex::from_string(&prev_pos).ok()?;
            FractionalIndex::new_between(&prev_fi, &before_fi)
                .map(|fi| fi.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Drag helper: Commit drop — optimistic update + server fn dispatch + rollback
// ---------------------------------------------------------------------------

/// Snapshot of a card's state before an optimistic move (for rollback).
#[derive(Clone)]
struct PreMoveSnapshot {
    from_list_id: String,
    original_position: String,
    original_done: bool,
    original_index: usize,
}

/// Execute the full drop: snapshot → optimistic update → server dispatch → rollback on error.
///
/// D-06 requirement: sets `done` optimistically based on `done_list_ids`.
fn commit_drop(
    info: DragInfo,
    board_signals: BoardSignals,
    drag_error: leptos::prelude::RwSignal<Option<String>>,
) {
    let card_id = info.card_id.clone();
    let from_list_id = info.from_list_id.clone();

    // --- Resolve the target list and before-card from hover state ---
    // hover_list_id was set by update_hover_target during pointermove.
    // If no hover, abort (pointer released outside a list area).
    let to_list_id = match board_signals.hover_list_id.get_untracked() {
        Some(id) => id,
        None => return,
    };

    // Resolve before-card from hover target (currently: append to hovered list).
    // Full hit-testing for before-card is implemented in update_hover_target_with_card below.
    // For now we use None (append) — the WASM path refines this via before_card_id signal.
    let before_card_id = board_signals.before_card_id.get_untracked();

    // --- (a) Capture pre-move snapshot ---
    let card_sig = match board_signals.card_signals.with(|cs| cs.get(&card_id).copied()) {
        Some(sig) => sig,
        None => return,
    };
    let pre_card = card_sig.get_untracked();
    let original_index = board_signals.list_cards.with(|m| {
        m.get(&from_list_id)
            .and_then(|ids| ids.iter().position(|id| id == &card_id))
            .unwrap_or(0)
    });
    let snapshot = PreMoveSnapshot {
        from_list_id: from_list_id.clone(),
        original_position: pre_card.position.clone(),
        original_done: pre_card.done,
        original_index,
    };

    // --- (b) Compute drop position (EXCLUDING dragged card from neighbor list) ---
    // Get the target list cards, excluding the dragged card
    let target_cards_raw = board_signals.list_cards.with(|m| {
        m.get(&to_list_id).cloned().unwrap_or_default()
    });
    let target_cards_excl: Vec<String> = target_cards_raw.iter()
        .filter(|id| *id != &card_id)
        .cloned()
        .collect();

    let new_position = board_signals.card_signals.with(|cs| {
        compute_card_drop_position(&target_cards_excl, cs, before_card_id.as_deref())
    });

    let new_position = match new_position {
        Some(p) => p,
        None => {
            // Key space exhausted — fallback to append after all
            board_signals.card_signals.with(|cs| {
                compute_card_drop_position(&target_cards_excl, cs, None)
            }).unwrap_or_else(|| fractional_index::FractionalIndex::default().to_string())
        }
    };

    // --- (c) Optimistic update ---
    let is_done_optimistic = board_signals.done_list_ids.get_untracked().contains(&to_list_id);
    card_sig.update(|c| {
        c.list_id = to_list_id.clone();
        c.position = new_position.clone();
        c.done = is_done_optimistic;
    });

    // Move card id between list_cards[from] → list_cards[to]
    board_signals.list_cards.update(|m| {
        // Remove from source list
        if let Some(ids) = m.get_mut(&from_list_id) {
            ids.retain(|id| id != &card_id);
        }
        // Insert into target list at the correct index
        let insert_idx = compute_insert_index(&target_cards_excl, before_card_id.as_deref());
        let target_ids = m.entry(to_list_id.clone()).or_default();
        if insert_idx <= target_ids.len() {
            target_ids.insert(insert_idx, card_id.clone());
        } else {
            target_ids.push(card_id.clone());
        }
    });

    // --- (d) Dispatch server fn ---
    let board_id = board_signals.board_id.get_untracked();
    let action = board_signals.move_card_action;
    action.dispatch(MoveCard {
        board_id,
        card_id: card_id.clone(),
        to_list_id: to_list_id.clone(),
        new_position: new_position.clone(),
    });

    // Capture the action version produced by THIS dispatch. `move_card_action` is a
    // single shared action whose `.value()` retains the previous dispatch's result until
    // the new one resolves, and the Effect below runs once on creation. Gating on an
    // exact version match ensures this Effect (a) ignores the stale pre-dispatch value,
    // and (b) stops acting once a newer drop supersedes it — preventing the per-drop
    // Effect accumulation from compounding rollbacks on a single error (CR-03).
    let my_version = action.version().get_untracked();

    // --- (e) Watch for error → rollback (scoped to this dispatch only) ---
    let card_id_for_rollback = card_id.clone();
    let to_list_id_for_rollback = to_list_id.clone();
    let snapshot_for_rollback = snapshot.clone();
    Effect::new(move |_| {
        // Only react to results belonging to this dispatch. A newer drop advances
        // `version()`, after which this Effect no longer matches and goes inert.
        if action.version().get() != my_version {
            return;
        }
        if let Some(Err(_)) = action.value().get() {
            // Rollback: restore card signal to pre-move state
            if let Some(sig) = board_signals.card_signals.with(|cs| cs.get(&card_id_for_rollback).copied()) {
                sig.update(|c| {
                    c.list_id = snapshot_for_rollback.from_list_id.clone();
                    c.position = snapshot_for_rollback.original_position.clone();
                    c.done = snapshot_for_rollback.original_done;
                });
            }
            // Rollback list_cards: remove from to_list, re-insert into from_list at original_index
            board_signals.list_cards.update(|m| {
                if let Some(ids) = m.get_mut(&to_list_id_for_rollback) {
                    ids.retain(|id| id != &card_id_for_rollback);
                }
                let from_ids = m.entry(snapshot_for_rollback.from_list_id.clone()).or_default();
                let idx = snapshot_for_rollback.original_index.min(from_ids.len());
                from_ids.insert(idx, card_id_for_rollback.clone());
            });
            drag_error.set(Some("Couldn't move card — changes reverted".to_string()));
        }
    });
}

/// Compute the index to insert the card into the target list's id vec.
fn compute_insert_index(
    target_cards_excl: &[String],
    before_card_id: Option<&str>,
) -> usize {
    match before_card_id {
        None => target_cards_excl.len(), // append
        Some(before_id) => {
            target_cards_excl.iter()
                .position(|id| id == before_id)
                .unwrap_or(target_cards_excl.len())
        }
    }
}

// ---------------------------------------------------------------------------
// AddCardComposer — inline rapid-entry card composer (CARD-01)
// ---------------------------------------------------------------------------

/// Inline composer for creating a new card at the end of a list.
///
/// Collapsed: a text button "Add a card".
/// Expanded: textarea + "Add card" button + close button.
///
/// Keyboard behavior (UI-SPEC §AddCardComposer):
/// - Enter (no Shift): prevent default → submit → clear → re-focus (composer stays open)
/// - Shift+Enter: newline (default behavior)
/// - Escape: close composer → revert to "Add a card" button
///
/// On submit: dispatches CreateCard server fn AND optimistically inserts into BoardSignals.
/// On server error: rolls back the optimistic card and surfaces a non-blocking error.
#[component]
pub fn AddCardComposer(
    list_id: String,
    board_id: String,
    board_signals: BoardSignals,
) -> impl IntoView {
    let composing = RwSignal::new(false);
    let error_msg = RwSignal::new(Option::<String>::None);
    let textarea_ref = NodeRef::<leptos::html::Textarea>::new();

    // Auto-focus textarea when composer opens (same pattern as AddListComposer)
    Effect::new(move |_| {
        if composing.get() {
            if let Some(ta) = textarea_ref.get() {
                let _ = ta.focus();
            }
        }
    });

    let list_id_sv = StoredValue::new(list_id.clone());
    let board_id_sv = StoredValue::new(board_id.clone());

    // ServerAction for CreateCard
    let create_action = ServerAction::<CreateCard>::new();

    // Watch for server errors to roll back optimistic card
    // We store the optimistic card ID so we can remove it on failure
    let optimistic_id: RwSignal<Option<String>> = RwSignal::new(None);

    Effect::new(move |_| {
        match create_action.value().get() {
            Some(Ok(card)) => {
                // Server confirmed: update the optimistic card signal with real data
                let real_id = card.id.clone();
                board_signals.card_signals.update(|cs| {
                    // Replace optimistic entry if it exists, or update with real card
                    if let Some(opt_id) = optimistic_id.get_untracked() {
                        if opt_id != real_id {
                            // Remove old optimistic entry, add real one
                            cs.remove(&opt_id);
                        }
                        // Update with confirmed server data
                        if let Some(sig) = cs.get(&real_id) {
                            sig.set(card.clone());
                        } else {
                            cs.insert(real_id.clone(), RwSignal::new(card.clone()));
                        }
                        // Fix list_cards too if id changed
                        if opt_id != real_id {
                            let lid = list_id_sv.get_value();
                            board_signals.list_cards.update(|m| {
                                if let Some(ids) = m.get_mut(&lid) {
                                    ids.retain(|id| id != &opt_id);
                                    if !ids.contains(&real_id) {
                                        ids.push(real_id.clone());
                                    }
                                }
                            });
                        }
                    }
                    optimistic_id.set(None);
                });
                error_msg.set(None);
            }
            Some(Err(_)) => {
                // Rollback: remove optimistic card
                if let Some(opt_id) = optimistic_id.get_untracked() {
                    let lid = list_id_sv.get_value();
                    board_signals.card_signals.update(|cs| { cs.remove(&opt_id); });
                    board_signals.list_cards.update(|m| {
                        if let Some(ids) = m.get_mut(&lid) {
                            ids.retain(|id| id != &opt_id);
                        }
                    });
                    optimistic_id.set(None);
                }
                error_msg.set(Some("Failed to add card — try again".to_string()));
            }
            None => {}
        }
    });

    // Submit: optimistic insert + dispatch server fn
    let submit = move |title: String| {
        let trimmed = title.trim().to_string();
        if trimmed.is_empty() { return; }

        let list_id_val = list_id_sv.get_value();
        let board_id_val = board_id_sv.get_value();

        // Optimistic insert: create a temporary Card with a client-side placeholder id.
        // Simple monotonic counter-based temp id — replaced by the real server id
        // when the server fn responds. Only needs to be unique within this session.
        let temp_id = {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(1);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            format!("opt-{}", n)
        };

        // Compute an optimistic position (append after last)
        let optimistic_pos = {
            use fractional_index::FractionalIndex;
            board_signals.list_cards.with(|m| {
                let ids = m.get(&list_id_val).cloned().unwrap_or_default();
                let last_pos = ids.last()
                    .and_then(|id| board_signals.card_signals.with(|cs| {
                        cs.get(id).map(|sig| sig.get_untracked().position.clone())
                    }));
                match last_pos {
                    None => FractionalIndex::default().to_string(),
                    Some(p) => FractionalIndex::from_string(&p)
                        .map(|fi| FractionalIndex::new_after(&fi).to_string())
                        .unwrap_or_else(|_| FractionalIndex::default().to_string()),
                }
            })
        };

        let optimistic_card = crate::models::Card {
            id: temp_id.clone(),
            list_id: list_id_val.clone(),
            board_id: board_id_val.clone(),
            card_num: 0, // will be replaced by server response
            title: trimmed.clone(),
            position: optimistic_pos,
            priority: None,
            due_at: None,
            done: false,
            archived: false,
            cover: None,
            labels: Vec::new(),
            checklist_done: 0,
            checklist_total: 0,
            comment_count: 0,
            attachment_count: 0,
            member_ids: Vec::new(),
        };

        // Insert optimistic card into signals
        board_signals.card_signals.update(|cs| {
            cs.insert(temp_id.clone(), RwSignal::new(optimistic_card));
        });
        board_signals.list_cards.update(|m| {
            m.entry(list_id_val.clone()).or_default().push(temp_id.clone());
        });
        optimistic_id.set(Some(temp_id));

        // Dispatch server fn
        create_action.dispatch(CreateCard {
            board_id: board_id_val,
            list_id: list_id_val,
            title: trimmed,
        });

        error_msg.set(None);
    };

    view! {
        <Show
            when=move || composing.get()
            fallback=move || view! {
                <button
                    type="button"
                    class="lns-add-card-btn"
                    on:click=move |_| composing.set(true)
                >
                    <crate::components::icon::Icon name="plus"/>
                    "Add a card"
                </button>
            }
        >
            <div class="lns-add-composer">
                // Error banner (non-blocking, dismisses on next submit attempt)
                <Show when=move || error_msg.get().is_some()>
                    <div class="lns-error-inline">
                        {move || error_msg.get().unwrap_or_default()}
                    </div>
                </Show>

                // Textarea
                <textarea
                    node_ref=textarea_ref
                    class="lns-add-composer-textarea"
                    placeholder="Enter a title for this card…"
                    rows="3"
                    on:keydown={
                        let s = submit.clone();
                        move |e: leptos::ev::KeyboardEvent| {
                            if e.key() == "Enter" && !e.shift_key() {
                                e.prevent_default();
                                if let Some(ta) = textarea_ref.get() {
                                    let val = ta.value();
                                    s(val);
                                    ta.set_value("");
                                    // Re-focus so composer stays open for rapid entry (CARD-01)
                                    let _ = ta.focus();
                                }
                            } else if e.key() == "Escape" {
                                composing.set(false);
                            }
                            // Shift+Enter: default newline behavior (no preventDefault)
                        }
                    }
                />

                // Action row
                <div class="lns-composer-row">
                    <button
                        type="button"
                        class="lns-btn lns-btn--primary lns-btn--sm"
                        on:click={
                            let s = submit.clone();
                            move |_| {
                                if let Some(ta) = textarea_ref.get() {
                                    let val = ta.value();
                                    s(val);
                                    ta.set_value("");
                                    let _ = ta.focus();
                                }
                            }
                        }
                    >
                        "Add card"
                    </button>
                    <button
                        type="button"
                        class="lns-btn lns-btn--ghost lns-btn--sm"
                        aria-label="Close composer"
                        on:click=move |_| composing.set(false)
                    >
                        <crate::components::icon::Icon name="x"/>
                    </button>
                </div>
            </div>
        </Show>
    }
}

// ---------------------------------------------------------------------------
// AddListComposer — inline "Add another list" composer
// ---------------------------------------------------------------------------

/// Inline composer for creating a new list at the end of the board canvas.
///
/// Collapsed: a dashed button "Add another list".
/// Expanded: text input + "Add list" button + "×" cancel.
/// Enter or "Add list" dispatches `CreateList`; Escape or "×" collapses. (D-16)
#[component]
pub fn AddListComposer(
    board_id: String,
    create_action: ServerAction<CreateList>,
) -> impl IntoView {
    let composing = RwSignal::new(false);
    let input_value = RwSignal::new(String::new());
    let composer_ref = NodeRef::<leptos::html::Input>::new();

    // Auto-focus input when composer opens
    Effect::new(move |_| {
        if composing.get() {
            if let Some(input) = composer_ref.get() {
                let _ = input.focus();
            }
        }
    });

    let board_id_sv = StoredValue::new(board_id);

    // Fn-compatible submit — reads StoredValue + signals each call
    let submit = move || {
        let name = input_value.get();
        let trimmed = name.trim().to_string();
        if !trimmed.is_empty() {
            create_action.dispatch(CreateList {
                board_id: board_id_sv.get_value(),
                name: trimmed,
            });
            input_value.set(String::new());
            composing.set(false);
        }
    };

    view! {
        <div class="lns-add-list-composer-wrap">
            <Show
                when=move || composing.get()
                fallback=move || view! {
                    <button
                        type="button"
                        class="lns-add-list-btn"
                        on:click=move |_| composing.set(true)
                    >
                        <crate::components::icon::Icon name="plus"/>
                        "Add another list"
                    </button>
                }
            >
                <div class="lns-add-list-composer">
                    <input
                        node_ref=composer_ref
                        type="text"
                        class="lns-input lns-add-list-input"
                        placeholder="List name…"
                        prop:value=move || input_value.get()
                        on:input=move |ev| input_value.set(event_target_value(&ev))
                        on:keydown={
                            let s = submit.clone();
                            move |e: leptos::ev::KeyboardEvent| {
                                match e.key().as_str() {
                                    "Enter" => s(),
                                    "Escape" => {
                                        input_value.set(String::new());
                                        composing.set(false);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    />
                    <div class="lns-add-list-composer-actions">
                        <button
                            type="button"
                            class="lns-btn lns-btn--sm"
                            on:click={
                                let s = submit.clone();
                                move |_| s()
                            }
                        >
                            "Add list"
                        </button>
                        <button
                            type="button"
                            class="lns-add-list-cancel"
                            aria-label="Cancel"
                            on:click=move |_| {
                                input_value.set(String::new());
                                composing.set(false);
                            }
                        >
                            "×"
                        </button>
                    </div>
                </div>
            </Show>
        </div>
    }
}

// ---------------------------------------------------------------------------
// EmptyBoardCard — first-run state when a board has no lists
// ---------------------------------------------------------------------------

/// First-run empty state displayed when a board has zero lists (BOARD-01, design screen 03).
///
/// Shows a centered 540px card with heading, CTAs, and template tiles.
/// Decorative ghost list rectangles render behind the card.
#[component]
pub fn EmptyBoardCard(
    on_add_list: Callback<()>,
    on_browse_templates: Callback<()>,
) -> impl IntoView {
    // Template tiles (visual — clicking routes through on_browse_templates)
    const TEMPLATE_TILES: &[(&str, &str, &str)] = &[
        ("Personal todos",  "#7c5cff", "Inbox · Today · Done"),
        ("Weekly review",   "#10b981", "Wins · Stuck · Next"),
        ("Trip planning",   "#0ea5e9", "Ideas · Booked · Day-of"),
    ];

    view! {
        // Decorative ghost list rectangles (aria-hidden)
        <div class="lns-empty-ghost-lists" aria-hidden="true">
            <div class="lns-empty-ghost-list lns-empty-ghost-list--1"/>
            <div class="lns-empty-ghost-list lns-empty-ghost-list--2"/>
            <div class="lns-empty-ghost-list lns-empty-ghost-list--3"/>
        </div>

        // Centered empty-board card
        <div class="lns-empty-board-card">
            // Icon tile
            <div class="lns-empty-board-icon-tile" aria-hidden="true">
                <crate::components::icon::Icon name="grid"/>
            </div>

            // Heading — 19px/700 (handoff-locked, README §03)
            <h2 class="lns-empty-board-heading">
                "Start with a blank board"
            </h2>
            <p class="lns-empty-board-subtitle">
                "Add your first list to start organizing."
            </p>

            // CTAs
            <div class="lns-empty-board-actions">
                <button
                    type="button"
                    class="lns-btn lns-btn--primary"
                    on:click=move |_| on_add_list.run(())
                >
                    "Add your first list"
                </button>
                <button
                    type="button"
                    class="lns-btn lns-btn--ghost"
                    on:click=move |_| on_browse_templates.run(())
                >
                    "Browse templates"
                </button>
            </div>

            // POPULAR TEMPLATES divider
            <div class="lns-empty-board-divider">
                <span class="lns-section-label">"POPULAR TEMPLATES"</span>
            </div>

            // Template tiles row
            <div class="lns-empty-board-templates">
                {TEMPLATE_TILES.iter().map(|&(name, color, preview)| {
                    let on_browse = on_browse_templates.clone();
                    view! {
                        <button
                            type="button"
                            class="lns-template-tile"
                            on:click=move |_| on_browse.run(())
                        >
                            <span
                                class="lns-template-tile-dot"
                                style=format!("background-color: {};", color)
                            />
                            <p class="lns-template-tile-name">{name}</p>
                            <p class="lns-template-tile-preview">{preview}</p>
                        </button>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}
