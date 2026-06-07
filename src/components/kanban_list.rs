use leptos::prelude::*;
use crate::models::{List, Card};
use crate::api::list_api::{CreateList, RenameList, ReorderList};

// ---------------------------------------------------------------------------
// CardStub — title-only card render (D-08, Phase 3 minimal stub)
// ---------------------------------------------------------------------------

/// A minimal card stub rendering the card title only (D-08).
/// Phase 4 will enrich this with cover images, labels, due dates, and meta.
/// No priority/due_at/label markup intentionally — this is a Phase 3 stub.
#[component]
pub fn CardStub(card: Card) -> impl IntoView {
    view! {
        <div class="lns-card lns-card-stub">
            <span class="lns-card-stub-title">{card.title}</span>
        </div>
    }
}

// ---------------------------------------------------------------------------
// KanbanList — full list component with rename, card stubs, overflow menu
// ---------------------------------------------------------------------------

/// A kanban list with inline rename, card stubs, and Move left/right reorder.
///
/// The caller passes fractional neighbor positions for reorder computation (Pattern 4).
/// Client-side computation of midpoints using fractional_index::FractionalIndex (D-15).
#[component]
pub fn KanbanList(
    list: List,
    cards: Vec<Card>,
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

    let card_count = cards.len();
    let list_name_display = list.name.clone();
    let list_name_input_default = list.name.clone();

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

                // Count pill
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
                <For
                    each=move || cards.clone()
                    key=|c| c.id.clone()
                    children=|card| view! { <CardStub card=card/> }
                />
            </div>

            // ── Footer: "Add a card" stub (Phase 4 activates) ────────────
            <div class="lns-list-footer">
                <button type="button" class="lns-add-card-btn" disabled=true>
                    <crate::components::icon::Icon name="plus"/>
                    "Add a card"
                </button>
            </div>
        </div>
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
