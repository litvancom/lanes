//! Checklist section component for the card-detail modal (Plan 03).
//!
//! Renders the reactive checklist with:
//! - Header: "Checklist · {done}/{total}" + "Hide completed" ghost button
//! - Progress bar (role=progressbar, fill = done/total %, animates)
//! - Item list with inline checkbox, optimistic toggle
//! - "Add an item" composer
//!
//! Optimistic updates: toggling an item immediately updates the modal-scoped
//! `RwSignal<Vec<ChecklistItem>>`, and on server fn success writes the returned
//! (done_count, total_count) through the per-card `RwSignal<Card>` so the board
//! thumbnail stays accurate without a full board refetch (D-15 / T-05-12).

use leptos::prelude::*;
use crate::models::{Card, ChecklistItem};
use crate::routes::board::BoardSignals;
use crate::api::card_detail_api::{ToggleChecklistItem, AddChecklistItem};
use crate::components::icon::Icon;

/// Checklist section: progress bar + item list + add-item composer.
///
/// Props:
/// - `board_id`, `card_id`: used when dispatching server actions
/// - `checklist_items`: modal-scoped signal seeded from CardDetail
/// - `card_signal_key`: card_id string to look up per-card RwSignal<Card> in BoardSignals
#[component]
pub fn ChecklistSection(
    board_id: String,
    card_id: String,
    checklist_items: RwSignal<Vec<ChecklistItem>>,
    card_signal_key: String,
) -> impl IntoView {
    let board_id = StoredValue::new(board_id);
    let card_id = StoredValue::new(card_id);
    let card_signal_key = StoredValue::new(card_signal_key);

    // Server actions
    let toggle_action = ServerAction::<ToggleChecklistItem>::new();
    let add_action = ServerAction::<AddChecklistItem>::new();

    // Board signals context for write-through to per-card RwSignal<Card>
    let board_signals: Option<BoardSignals> = use_context::<BoardSignals>();

    // Derived reactive counts from the modal-scoped signal
    let done_count = move || {
        checklist_items.with(|items| items.iter().filter(|i| i.done).count() as i64)
    };
    let total_count = move || checklist_items.with(|items| items.len() as i64);

    // Hide-completed toggle
    let hide_completed = RwSignal::new(false);

    // Add-item composer state
    let add_text = RwSignal::new(String::new());
    let adding = RwSignal::new(false);

    // Snapshot of the (item_id, applied done) of the in-flight toggle, for revert-on-error (WR-02).
    let toggle_snapshot: RwSignal<Option<(String, bool)>> = RwSignal::new(None);
    // Inline error surfaced when a checklist mutation fails.
    let checklist_error: RwSignal<Option<String>> = RwSignal::new(None);

    // On toggle_action success: write-through authoritative card counts (D-15).
    // On error: revert the optimistic item flip and surface the message (WR-02).
    {
        let bs = board_signals;
        Effect::new(move |_| {
            if let Some(result) = toggle_action.value().get() {
                match result {
                    Ok((done_flag, done_c, total_c)) => {
                        let _ = done_flag;
                        toggle_snapshot.set(None);
                        checklist_error.set(None);
                        if let Some(bs_ref) = bs {
                            let key = card_signal_key.get_value();
                            bs_ref.card_signals.with(|cs| {
                                if let Some(sig) = cs.get(&key) {
                                    sig.update(|c: &mut Card| {
                                        c.checklist_done = done_c;
                                        c.checklist_total = total_c;
                                    });
                                }
                            });
                        }
                    }
                    Err(_) => {
                        // Revert the optimistic flip on the affected item
                        if let Some((iid, applied)) = toggle_snapshot.get_untracked() {
                            checklist_items.update(|items| {
                                if let Some(i) = items.iter_mut().find(|i| i.id == iid) {
                                    i.done = !applied;
                                }
                            });
                        }
                        toggle_snapshot.set(None);
                        checklist_error.set(Some("Couldn't save changes. Try again.".to_string()));
                    }
                }
            }
        });
    }

    // On add_action success: append returned item + write-through card total (D-15).
    // The add path is non-optimistic (appends only on success), so an error needs no
    // revert — just surface the message (WR-02).
    {
        let bs = board_signals;
        Effect::new(move |_| {
            match add_action.value().get() {
                Some(Ok((new_item, done_c, total_c))) => {
                checklist_error.set(None);
                checklist_items.update(|items| items.push(new_item));
                add_text.set(String::new());
                adding.set(false);
                if let Some(bs_ref) = bs {
                    let key = card_signal_key.get_value();
                    bs_ref.card_signals.with(|cs| {
                        if let Some(sig) = cs.get(&key) {
                            sig.update(|c: &mut Card| {
                                c.checklist_done = done_c;
                                c.checklist_total = total_c;
                            });
                        }
                    });
                }
                }
                Some(Err(_)) => {
                    checklist_error.set(Some("Couldn't save changes. Try again.".to_string()));
                }
                None => {}
            }
        });
    }

    view! {
        <Show when=move || { total_count() > 0 || adding.get() }>
            <div>
                // ── Header ──────────────────────────────────────────────────
                <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 0">
                    <h4 style="margin: 0">
                        <Icon name="check"/>
                        " Checklist · "
                        {move || format!("{}/{}", done_count(), total_count())}
                    </h4>
                    <Show when=move || { total_count() > 0 }>
                        <button
                            class="lns-btn lns-btn--ghost lns-btn--sm"
                            on:click=move |_| hide_completed.update(|v| *v = !*v)
                        >
                            {move || if hide_completed.get() { "Show completed" } else { "Hide completed" }}
                        </button>
                    </Show>
                </div>

                // ── Progress bar ─────────────────────────────────────────────
                <Show when=move || { total_count() > 0 }>
                    {move || {
                        let done = done_count();
                        let total = total_count();
                        let pct = if total > 0 { (done as f64 / total as f64 * 100.0) as u32 } else { 0 };
                        view! {
                            <div
                                class="lns-checklist-bar"
                                role="progressbar"
                                aria-valuenow=pct.to_string()
                                aria-valuemax="100"
                                aria-label={move || format!("{}/{} checklist items done", done_count(), total_count())}
                            >
                                <div style=format!("width: {}%", pct)/>
                            </div>
                        }
                    }}
                </Show>

                // ── Item list ────────────────────────────────────────────────
                <div class="lns-checklist">
                    {move || {
                        checklist_items.with(|items| {
                            items.iter().filter(|item| {
                                // When hide_completed is on, filter out done items
                                !(hide_completed.get() && item.done)
                            }).map(|item| {
                                let item_id = StoredValue::new(item.id.clone());
                                let is_done = item.done;
                                let text = item.text.clone();

                                view! {
                                    <div
                                        class=format!("lns-checklist-item{}", if is_done { " done" } else { "" })
                                        role="checkbox"
                                        aria-checked=if is_done { "true" } else { "false" }
                                        tabindex="0"
                                        on:click={
                                            let bid = board_id.get_value();
                                            let cid = card_id.get_value();
                                            move |_| {
                                                let iid = item_id.get_value();
                                                let new_done = !is_done;
                                                // Optimistic: flip item in modal signal,
                                                // recording it for revert-on-error (WR-02)
                                                toggle_snapshot.set(Some((iid.clone(), new_done)));
                                                checklist_items.update(|items| {
                                                    if let Some(i) = items.iter_mut().find(|i| i.id == iid) {
                                                        i.done = new_done;
                                                    }
                                                });
                                                toggle_action.dispatch(ToggleChecklistItem {
                                                    board_id: bid.clone(),
                                                    card_id: cid.clone(),
                                                    item_id: iid,
                                                    done: new_done,
                                                });
                                            }
                                        }
                                        on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                            if ev.key() == " " || ev.key() == "Enter" {
                                                ev.prevent_default();
                                                let iid = item_id.get_value();
                                                let new_done = !is_done;
                                                toggle_snapshot.set(Some((iid.clone(), new_done)));
                                                checklist_items.update(|items| {
                                                    if let Some(i) = items.iter_mut().find(|i| i.id == iid) {
                                                        i.done = new_done;
                                                    }
                                                });
                                                toggle_action.dispatch(ToggleChecklistItem {
                                                    board_id: board_id.get_value(),
                                                    card_id: card_id.get_value(),
                                                    item_id: iid,
                                                    done: new_done,
                                                });
                                            }
                                        }
                                    >
                                        <span class="lns-check" aria-hidden="true">
                                            <Show when=move || is_done>
                                                <Icon name="check"/>
                                            </Show>
                                        </span>
                                        <span>{text}</span>
                                    </div>
                                }
                            }).collect_view()
                        })
                    }}
                </div>

                // ── Add an item composer ──────────────────────────────────────
                <Show
                    when=move || adding.get()
                    fallback=move || {
                        view! {
                            <button
                                class="lns-add-card-btn"
                                style="margin-top: 4px; font-size: 13px"
                                on:click=move |_| adding.set(true)
                            >
                                "Add an item"
                            </button>
                        }
                    }
                >
                    <div style="margin-top: 4px; display: flex; flex-direction: column; gap: 6px">
                        <input
                            type="text"
                            class="lns-modal-title-input"
                            style="font-size: 13px; padding: 6px 8px"
                            placeholder="Add an item…"
                            prop:value=move || add_text.get()
                            on:input=move |ev| add_text.set(event_target_value(&ev))
                            on:keydown={
                                let bid = board_id.get_value();
                                let cid = card_id.get_value();
                                move |ev: leptos::ev::KeyboardEvent| {
                                    match ev.key().as_str() {
                                        "Enter" => {
                                            let t = add_text.get_untracked();
                                            if !t.trim().is_empty() {
                                                add_action.dispatch(AddChecklistItem {
                                                    board_id: bid.clone(),
                                                    card_id: cid.clone(),
                                                    text: t,
                                                });
                                            }
                                        }
                                        "Escape" => {
                                            add_text.set(String::new());
                                            adding.set(false);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            autofocus
                        />
                        <div style="display: flex; gap: 6px">
                            <button
                                class="lns-btn lns-btn--primary lns-btn--sm"
                                on:click={
                                    let bid = board_id.get_value();
                                    let cid = card_id.get_value();
                                    move |_| {
                                        let t = add_text.get_untracked();
                                        if !t.trim().is_empty() {
                                            add_action.dispatch(AddChecklistItem {
                                                board_id: bid.clone(),
                                                card_id: cid.clone(),
                                                text: t,
                                            });
                                        }
                                    }
                                }
                            >
                                "Add"
                            </button>
                            <button
                                class="lns-btn lns-btn--ghost lns-btn--sm"
                                on:click=move |_| {
                                    add_text.set(String::new());
                                    adding.set(false);
                                }
                            >
                                "Cancel"
                            </button>
                        </div>
                    </div>
                </Show>
                {move || checklist_error.get().map(|msg| view! {
                    <div style="margin-top: 6px; font-size: 11px; color: var(--danger, #c0392b)">
                        {msg}
                    </div>
                })}
            </div>
        </Show>

        // Show the "Add an item" button even when no items exist (before first add)
        <Show when=move || { total_count() == 0 && !adding.get() }>
            <button
                class="lns-add-card-btn"
                style="margin-top: 4px; font-size: 13px"
                on:click=move |_| adding.set(true)
            >
                "Add an item"
            </button>
        </Show>
    }
}
