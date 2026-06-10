use leptos::prelude::*;
use crate::models::List;
use crate::api::list_api::{CreateList, RenameList, ReorderList};
use crate::api::card_api::CreateCard;
use crate::routes::board::BoardSignals;
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
        <div class="lns-list">
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
