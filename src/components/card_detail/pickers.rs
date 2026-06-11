//! Property picker popovers for the card-detail modal (Plan 03).
//!
//! Each picker is a `Show`-gated absolute-positioned popover triggered by the
//! corresponding sidebar button. All pickers write optimistically through
//! `RwSignal<Card>` and dispatch a server action to persist.
//!
//! Components:
//! - `LabelPicker`: toggles labels from the board's label list
//! - `DatePicker`: native `<input type="date">` for due_at
//! - `PriorityPicker`: P1 / P2 / P3 / None selector
//! - `MemberPicker`: assigns/removes board members; auto-watch happens server-side

use leptos::prelude::*;
use crate::models::{Card, CardLabel, UserSummary};
use crate::routes::board::BoardSignals;
use crate::api::card_detail_api::{AssignLabel, SetDueDate, SetPriority, AssignMember, RemoveMember};
use crate::components::icon::Icon;

// ---------------------------------------------------------------------------
// Helper: format epoch millis → "YYYY-MM-DD" for <input type="date">
// ---------------------------------------------------------------------------

fn millis_to_date_string(ms: i64) -> String {
    let days = (ms / 1000) / 86400;
    let (y, m, d) = epoch_days_to_ymd(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Minimal Gregorian conversion: epoch days → (year, month, day).
fn epoch_days_to_ymd(days: i64) -> (i64, i64, i64) {
    // Algorithm: https://howardhinnant.github.io/date_algorithms.html#civil_from_days
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Parse "YYYY-MM-DD" → epoch millis (midnight UTC).
fn date_string_to_millis(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 { return None; }
    let y: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    if m < 1 || m > 12 || d < 1 || d > 31 { return None; }
    // Days since epoch: https://howardhinnant.github.io/date_algorithms.html#days_from_civil
    let m2 = m;
    let y2 = if m2 <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let doy = (153 * (if m2 > 2 { m2 - 3 } else { m2 + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 * 1000)
}

// ---------------------------------------------------------------------------
// LabelPicker
// ---------------------------------------------------------------------------

/// Popover showing board labels; toggling assigns/unassigns on the card.
///
/// Props:
/// - `board_id`, `card_id`: for server action dispatch
/// - `board_labels`: all labels on this board (passed from CardDetail.board_members analog)
/// - `card_signal_key`: key into BoardSignals for write-through
/// - `show`: RwSignal<bool> to open/close the popover
#[component]
pub fn LabelPicker(
    board_id: String,
    card_id: String,
    board_labels: Vec<CardLabel>,
    card_signal_key: String,
    show: RwSignal<bool>,
) -> impl IntoView {
    let board_id = StoredValue::new(board_id);
    let card_id = StoredValue::new(card_id);
    let card_signal_key = StoredValue::new(card_signal_key);
    let board_labels = StoredValue::new(board_labels);
    let assign_action = ServerAction::<AssignLabel>::new();
    let board_signals: Option<BoardSignals> = use_context::<BoardSignals>();

    // On success: toggle the label in RwSignal<Card>.labels
    {
        let bs = board_signals;
        Effect::new(move |_| {
            if let Some(Ok(())) = assign_action.value().get() {
                // Write-through happens below via optimistic; action success is a no-op here
                let _ = bs;
            }
        });
    }

    view! {
        <Show when=move || show.get()>
            // Overlay to close on outside click
            <div
                style="position: fixed; inset: 0; z-index: 200"
                on:click=move |_| show.set(false)
            />
            <div
                class="lns-picker-popover"
                style="position: absolute; z-index: 201; right: 0; top: 100%; min-width: 180px; background: var(--bg-elevated); border: 1px solid var(--border); border-radius: var(--radius-sm); box-shadow: var(--shadow-lg); padding: 8px 0"
            >
                <div style="font-size: 11px; font-weight: 600; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.04em; padding: 0 10px 6px">
                    "Labels"
                </div>
                {move || {
                    board_labels.with_value(|labels| {
                        labels.iter().map(|label| {
                            let label_id = StoredValue::new(label.id.clone());
                            let color = label.color.clone();
                            let name = label.name.clone();

                            view! {
                                <button
                                    style="display: flex; align-items: center; gap: 8px; width: 100%; padding: 6px 10px; background: none; border: none; cursor: pointer; font-size: 13px; text-align: left"
                                    style:color="var(--text)"
                                    on:click={
                                        let bid = board_id.get_value();
                                        let cid = card_id.get_value();
                                        let lid = label_id.get_value();
                                        let lid2 = lid.clone();
                                        let lcolor = color.clone();
                                        let lname = name.clone();
                                        move |ev| {
                                            ev.stop_propagation();
                                            // Read current assigned state from card signal
                                            let currently_assigned = board_signals
                                                .and_then(|bs| {
                                                    let key = card_signal_key.get_value();
                                                    bs.card_signals.with(|cs| {
                                                        cs.get(&key).map(|sig| {
                                                            sig.get().labels.iter().any(|l| l.id == lid2)
                                                        })
                                                    })
                                                })
                                                .unwrap_or(false);

                                            let new_assigned = !currently_assigned;

                                            // Optimistic write-through to RwSignal<Card>.labels
                                            if let Some(bs) = board_signals {
                                                let key = card_signal_key.get_value();
                                                let lid_c = lid2.clone();
                                                let lcolor_c = lcolor.clone();
                                                let lname_c = lname.clone();
                                                bs.card_signals.with(|cs| {
                                                    if let Some(sig) = cs.get(&key) {
                                                        sig.update(|c: &mut Card| {
                                                            if new_assigned {
                                                                if !c.labels.iter().any(|l| l.id == lid_c) {
                                                                    c.labels.push(CardLabel {
                                                                        id: lid_c.clone(),
                                                                        name: lname_c.clone(),
                                                                        color: lcolor_c.clone(),
                                                                    });
                                                                }
                                                            } else {
                                                                c.labels.retain(|l| l.id != lid_c);
                                                            }
                                                        });
                                                    }
                                                });
                                            }

                                            assign_action.dispatch(AssignLabel {
                                                board_id: bid.clone(),
                                                card_id: cid.clone(),
                                                label_id: lid.clone(),
                                                assigned: new_assigned,
                                            });
                                        }
                                    }
                                >
                                    <span
                                        class="lns-label"
                                        style=format!("background: {}; height: 12px; width: 40px; border-radius: 3px; flex-shrink: 0", color)
                                    />
                                    <span>{name.clone()}</span>
                                    // Checkmark if assigned (read from card signal)
                                    {move || {
                                        let lid_check = label_id.get_value();
                                        let assigned = board_signals
                                            .and_then(|bs| {
                                                let key = card_signal_key.get_value();
                                                bs.card_signals.with(|cs| {
                                                    cs.get(&key).map(|sig| {
                                                        sig.get().labels.iter().any(|l| l.id == lid_check)
                                                    })
                                                })
                                            })
                                            .unwrap_or(false);
                                        if assigned {
                                            view! { <Icon name="check"/> }.into_any()
                                        } else {
                                            view! { <span/> }.into_any()
                                        }
                                    }}
                                </button>
                            }
                        }).collect_view()
                    })
                }}
            </div>
        </Show>
    }
}

// ---------------------------------------------------------------------------
// DatePicker
// ---------------------------------------------------------------------------

/// Popover with a native <input type="date"> for the card's due_at.
#[component]
pub fn DatePicker(
    board_id: String,
    card_id: String,
    card_signal_key: String,
    show: RwSignal<bool>,
) -> impl IntoView {
    let board_id = StoredValue::new(board_id);
    let card_id = StoredValue::new(card_id);
    let card_signal_key = StoredValue::new(card_signal_key);
    let set_due_action = ServerAction::<SetDueDate>::new();
    let board_signals: Option<BoardSignals> = use_context::<BoardSignals>();

    // Current due_at from card signal for controlled input
    let current_date_str = move || {
        board_signals
            .and_then(|bs| {
                let key = card_signal_key.get_value();
                bs.card_signals.with(|cs| {
                    cs.get(&key).map(|sig| {
                        sig.get().due_at.map(millis_to_date_string).unwrap_or_default()
                    })
                })
            })
            .unwrap_or_default()
    };

    view! {
        <Show when=move || show.get()>
            <div
                style="position: fixed; inset: 0; z-index: 200"
                on:click=move |_| show.set(false)
            />
            <div
                class="lns-picker-popover"
                style="position: absolute; z-index: 201; right: 0; top: 100%; background: var(--bg-elevated); border: 1px solid var(--border); border-radius: var(--radius-sm); box-shadow: var(--shadow-lg); padding: 12px"
            >
                <div style="font-size: 11px; font-weight: 600; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.04em; margin-bottom: 8px">
                    "Due date"
                </div>
                <input
                    type="date"
                    style="border: 1px solid var(--border); border-radius: var(--radius-xs); padding: 5px 8px; font: inherit; font-size: 13px; background: var(--bg); color: var(--text); cursor: pointer"
                    prop:value=current_date_str
                    on:change={
                        let bid = board_id.get_value();
                        let cid = card_id.get_value();
                        move |ev| {
                            let val = event_target_value(&ev);
                            let new_due = if val.is_empty() {
                                None
                            } else {
                                date_string_to_millis(&val)
                            };

                            // Optimistic write-through to RwSignal<Card>.due_at
                            if let Some(bs) = board_signals {
                                let key = card_signal_key.get_value();
                                bs.card_signals.with(|cs| {
                                    if let Some(sig) = cs.get(&key) {
                                        sig.update(|c: &mut Card| {
                                            c.due_at = new_due;
                                        });
                                    }
                                });
                            }

                            set_due_action.dispatch(SetDueDate {
                                board_id: bid.clone(),
                                card_id: cid.clone(),
                                due_at: new_due,
                            });
                            show.set(false);
                        }
                    }
                    on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                />
                <div style="margin-top: 8px">
                    <button
                        class="lns-btn lns-btn--ghost lns-btn--sm"
                        on:click={
                            let bid = board_id.get_value();
                            let cid = card_id.get_value();
                            move |ev| {
                                ev.stop_propagation();
                                if let Some(bs) = board_signals {
                                    let key = card_signal_key.get_value();
                                    bs.card_signals.with(|cs| {
                                        if let Some(sig) = cs.get(&key) {
                                            sig.update(|c: &mut Card| c.due_at = None);
                                        }
                                    });
                                }
                                set_due_action.dispatch(SetDueDate {
                                    board_id: bid.clone(),
                                    card_id: cid.clone(),
                                    due_at: None,
                                });
                                show.set(false);
                            }
                        }
                    >
                        "Remove date"
                    </button>
                </div>
            </div>
        </Show>
    }
}

// ---------------------------------------------------------------------------
// PriorityPicker
// ---------------------------------------------------------------------------

/// Popover with P1 / P2 / P3 / None options.
#[component]
pub fn PriorityPicker(
    board_id: String,
    card_id: String,
    card_signal_key: String,
    show: RwSignal<bool>,
) -> impl IntoView {
    let board_id = StoredValue::new(board_id);
    let card_id = StoredValue::new(card_id);
    let card_signal_key = StoredValue::new(card_signal_key);
    let set_prio_action = ServerAction::<SetPriority>::new();
    let board_signals: Option<BoardSignals> = use_context::<BoardSignals>();

    let options: &'static [(&'static str, Option<&'static str>)] = &[
        ("P1 · High", Some("P1")),
        ("P2 · Medium", Some("P2")),
        ("P3 · Low", Some("P3")),
        ("None", None),
    ];

    view! {
        <Show when=move || show.get()>
            <div
                style="position: fixed; inset: 0; z-index: 200"
                on:click=move |_| show.set(false)
            />
            <div
                class="lns-picker-popover"
                style="position: absolute; z-index: 201; right: 0; top: 100%; min-width: 160px; background: var(--bg-elevated); border: 1px solid var(--border); border-radius: var(--radius-sm); box-shadow: var(--shadow-lg); padding: 8px 0"
            >
                <div style="font-size: 11px; font-weight: 600; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.04em; padding: 0 10px 6px">
                    "Priority"
                </div>
                {options.iter().map(|(label, prio_val)| {
                    let label = *label;
                    let prio_val: Option<String> = prio_val.map(|s| s.to_string());
                    let prio_val2 = prio_val.clone();
                    let cls = match prio_val.as_deref() {
                        Some("P1") => "lns-card-prio p1",
                        Some("P2") => "lns-card-prio p2",
                        Some("P3") => "lns-card-prio",
                        _ => "",
                    };

                    view! {
                        <button
                            style="display: flex; align-items: center; gap: 8px; width: 100%; padding: 6px 10px; background: none; border: none; cursor: pointer; font-size: 13px"
                            on:click={
                                let bid = board_id.get_value();
                                let cid = card_id.get_value();
                                let pv = prio_val2.clone();
                                move |ev| {
                                    ev.stop_propagation();
                                    let pv_clone = pv.clone();
                                    // Optimistic write-through
                                    if let Some(bs) = board_signals {
                                        let key = card_signal_key.get_value();
                                        bs.card_signals.with(|cs| {
                                            if let Some(sig) = cs.get(&key) {
                                                sig.update(|c: &mut Card| c.priority = pv_clone.clone());
                                            }
                                        });
                                    }
                                    set_prio_action.dispatch(SetPriority {
                                        board_id: bid.clone(),
                                        card_id: cid.clone(),
                                        priority: pv.clone(),
                                    });
                                    show.set(false);
                                }
                            }
                        >
                            <Show when=move || cls.is_empty() fallback=move || view! {
                                <span class=cls style="font-size: 11px; padding: 2px 6px">{label}</span>
                            }>
                                <span style="color: var(--text-muted)">{label}</span>
                            </Show>
                            // Checkmark for currently selected
                            {move || {
                                let pv_check = prio_val.clone();
                                let current = board_signals
                                    .and_then(|bs| {
                                        let key = card_signal_key.get_value();
                                        bs.card_signals.with(|cs| {
                                            cs.get(&key).map(|sig| sig.get().priority.clone())
                                        })
                                    })
                                    .flatten();
                                if current.as_deref() == pv_check.as_deref() {
                                    view! { <Icon name="check"/> }.into_any()
                                } else {
                                    view! { <span/> }.into_any()
                                }
                            }}
                        </button>
                    }
                }).collect_view()}
            </div>
        </Show>
    }
}

// ---------------------------------------------------------------------------
// MemberPicker
// ---------------------------------------------------------------------------

/// Popover showing board members; toggle assigns/removes from card.
/// Assigning auto-watches the card (server-side, D-12).
#[component]
pub fn MemberPicker(
    board_id: String,
    card_id: String,
    board_members: Vec<UserSummary>,
    card_signal_key: String,
    show: RwSignal<bool>,
) -> impl IntoView {
    let board_id = StoredValue::new(board_id);
    let card_id = StoredValue::new(card_id);
    let card_signal_key = StoredValue::new(card_signal_key);
    let board_members = StoredValue::new(board_members);
    let assign_action = ServerAction::<AssignMember>::new();
    let remove_action = ServerAction::<RemoveMember>::new();
    let board_signals: Option<BoardSignals> = use_context::<BoardSignals>();

    view! {
        <Show when=move || show.get()>
            <div
                style="position: fixed; inset: 0; z-index: 200"
                on:click=move |_| show.set(false)
            />
            <div
                class="lns-picker-popover"
                style="position: absolute; z-index: 201; right: 0; top: 100%; min-width: 200px; background: var(--bg-elevated); border: 1px solid var(--border); border-radius: var(--radius-sm); box-shadow: var(--shadow-lg); padding: 8px 0"
            >
                <div style="font-size: 11px; font-weight: 600; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.04em; padding: 0 10px 6px">
                    "Members"
                </div>
                {move || {
                    board_members.with_value(|members| {
                        members.iter().map(|member| {
                            let user_id = StoredValue::new(member.id.clone());
                            let display_name = member.display_name.clone();
                            let avatar_color = member.avatar_color.clone();

                            view! {
                                <button
                                    style="display: flex; align-items: center; gap: 8px; width: 100%; padding: 6px 10px; background: none; border: none; cursor: pointer; font-size: 13px; text-align: left"
                                    on:click={
                                        let bid = board_id.get_value();
                                        let cid = card_id.get_value();
                                        let uid = user_id.get_value();
                                        let uid2 = uid.clone();
                                        move |ev| {
                                            ev.stop_propagation();
                                            let currently_assigned = board_signals
                                                .and_then(|bs| {
                                                    let key = card_signal_key.get_value();
                                                    bs.card_signals.with(|cs| {
                                                        cs.get(&key).map(|sig| {
                                                            sig.get().member_ids.contains(&uid2)
                                                        })
                                                    })
                                                })
                                                .unwrap_or(false);

                                            // Optimistic write-through to RwSignal<Card>.member_ids
                                            if let Some(bs) = board_signals {
                                                let key = card_signal_key.get_value();
                                                let uid_c = uid2.clone();
                                                bs.card_signals.with(|cs| {
                                                    if let Some(sig) = cs.get(&key) {
                                                        sig.update(|c: &mut Card| {
                                                            if currently_assigned {
                                                                c.member_ids.retain(|id| id != &uid_c);
                                                            } else if !c.member_ids.contains(&uid_c) {
                                                                c.member_ids.push(uid_c.clone());
                                                            }
                                                        });
                                                    }
                                                });
                                            }

                                            if currently_assigned {
                                                remove_action.dispatch(RemoveMember {
                                                    board_id: bid.clone(),
                                                    card_id: cid.clone(),
                                                    user_id: uid.clone(),
                                                });
                                            } else {
                                                assign_action.dispatch(AssignMember {
                                                    board_id: bid.clone(),
                                                    card_id: cid.clone(),
                                                    user_id: uid.clone(),
                                                });
                                            }
                                        }
                                    }
                                >
                                    // Avatar circle
                                    <div
                                        class="lns-avatar lns-avatar--sm"
                                        style=format!("background: {}", avatar_color)
                                        aria-hidden="true"
                                    />
                                    <span style="color: var(--text)">{display_name.clone()}</span>
                                    // Checkmark if assigned
                                    {move || {
                                        let uid_check = user_id.get_value();
                                        let assigned = board_signals
                                            .and_then(|bs| {
                                                let key = card_signal_key.get_value();
                                                bs.card_signals.with(|cs| {
                                                    cs.get(&key).map(|sig| {
                                                        sig.get().member_ids.contains(&uid_check)
                                                    })
                                                })
                                            })
                                            .unwrap_or(false);
                                        if assigned {
                                            view! { <Icon name="check"/> }.into_any()
                                        } else {
                                            view! { <span/> }.into_any()
                                        }
                                    }}
                                </button>
                            }
                        }).collect_view()
                    })
                }}
            </div>
        </Show>
    }
}
