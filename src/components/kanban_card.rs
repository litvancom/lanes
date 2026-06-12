use leptos::prelude::*;
use crate::models::Card;
use crate::components::label_chip::LabelChip;
use crate::routes::board::{DragInfo, BoardSignals};

/// Validate a CSS color string for safe interpolation into inline styles (T-04-06).
/// Accepts #rrggbb, #rgb, and oklch(...) shapes. Falls back to transparent/neutral.
fn safe_cover_color(c: &str) -> &str {
    let s = c.trim();
    // Accept oklch(...) only if the interior is restricted to a numeric charset.
    // An unconstrained interior would still allow CSS-declaration injection inside the
    // style attribute (e.g. closing the function and appending @import) even though
    // Leptos escapes the attribute itself (WR-01).
    if let Some(inner) = s.strip_prefix("oklch(").and_then(|x| x.strip_suffix(')')) {
        if inner.chars().all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '%' | ' ' | '+' | '-')) {
            return c;
        }
    }
    if s.starts_with('#') && (s.len() == 7 || s.len() == 4)
        && s[1..].chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return c;
    }
    "transparent"
}

/// Current Unix time in milliseconds, resolved in a WASM-safe way.
///
/// `std::time::SystemTime::now()` panics on `wasm32-unknown-unknown`
/// ("time not implemented on this platform"), which crashed board hydration.
/// On the server we use `SystemTime`; in the browser client we use the JS clock.
fn current_unix_millis() -> i64 {
    #[cfg(feature = "ssr")]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        return SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
    }
    #[cfg(all(not(feature = "ssr"), feature = "hydrate"))]
    {
        // js_sys::Date::now() returns f64 millis since the Unix epoch.
        return js_sys::Date::now() as i64;
    }
    #[cfg(all(not(feature = "ssr"), not(feature = "hydrate")))]
    {
        // No-feature builds (e.g. rust-analyzer / bare `cargo check`) never run this.
        return 0;
    }
}

/// Format the due-at epoch millis into a human label and tone class.
///
/// Rules (mirrors `formatDue` in `components.jsx`):
/// - `card.done`: due chip hidden (caller should not call this when done=true)
/// - due_at < today midnight: tone "due-overdue", label = e.g. "May 20"
/// - due_at within 3 days from today: tone "due-soon"
/// - otherwise: tone "" (default muted color)
///
/// Returns `(label, tone_class)`. `tone_class` is one of "", "due-overdue", "due-soon".
pub fn format_due(due_at_ms: i64) -> (String, &'static str) {
    let now_ms = current_unix_millis();

    // Today's midnight UTC (start of today)
    let ms_per_day: i64 = 86_400_000;
    let today_midnight = (now_ms / ms_per_day) * ms_per_day;
    let three_days_later = today_midnight + 3 * ms_per_day;

    let tone = if due_at_ms < today_midnight {
        "due-overdue"
    } else if due_at_ms < three_days_later {
        "due-soon"
    } else {
        ""
    };

    // Format as "Mon DD" using simple epoch math.
    // We use chrono-free formatting: just compute day/month from millis.
    // Floor-divide so pre-epoch (negative) inputs map to the correct day rather than
    // truncating toward zero.
    let secs = due_at_ms.div_euclid(1000);
    let days_since_epoch = secs.div_euclid(86400);

    // Simple Gregorian calendar calculation
    let label = epoch_days_to_mon_day(days_since_epoch);

    (label, tone)
}

/// Convert days since Unix epoch (1970-01-01) to "Mon DD" format (e.g. "May 20").
///
/// Operates on signed `i64` arithmetic so malformed/pre-epoch inputs cannot underflow
/// and panic in debug builds (WR-03). The Julian Day Number offset (2440588) keeps the
/// intermediates positive for any realistic due date; the month index is bounds-checked
/// before indexing MONTHS.
fn epoch_days_to_mon_day(days: i64) -> String {
    // Algorithm: compute year/month/day from Julian day number
    // Days since epoch = days since 1970-01-01
    let jdn = days + 2440588; // Julian Day Number offset from 1970-01-01
    if jdn < 0 {
        // Far-pre-epoch / garbage input: the Meeus algorithm assumes a non-negative
        // Julian day. Bail out with a placeholder rather than computing nonsense.
        return "?".to_string();
    }

    // Gregorian calendar algorithm (Meeus, Astronomical Algorithms)
    let p = jdn + 68569;
    let q = 4 * p / 146097;
    let r = p - (146097 * q + 3) / 4;
    let s = 4000 * (r + 1) / 1461001;
    let t = r - 1461 * s / 4 + 31;
    let u = 80 * t / 2447;
    let day = t - 2447 * u / 80;
    let v = u / 11;
    let month = u + 2 - 12 * v;

    const MONTHS: &[&str] = &[
        "", "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let month_name = if (1..=12).contains(&month) {
        MONTHS[month as usize]
    } else {
        "?"
    };
    format!("{} {}", month_name, day)
}

/// Full kanban card thumbnail (CARD-07).
///
/// Renders the complete design spec card from a reactive `RwSignal<Card>`:
/// - Optional cover band
/// - Labels row (LabelChip bars/pills)
/// - Title (with done strikethrough)
/// - Meta row: priority pill, due chip, done badge, checklist, comments, attachments, avatars
///
/// `data-card-id` and `data-list-id` attributes are included for Plan 03 hit-testing.
/// `drag_info` prop enables Plan 03 pointer-events drag layer (additive).
#[component]
pub fn KanbanCard(
    card: RwSignal<Card>,
    labels_expanded: RwSignal<bool>,
    list_id: String,
    drag_info: RwSignal<Option<DragInfo>>,
) -> impl IntoView {
    let list_id_clone = list_id.clone();
    let list_id_for_drag = list_id.clone();

    let board_id_for_click = card.get_untracked().board_id.clone();

    // Phase 6: realtime highlight and fade-collapse class bindings (D-04/D-05/D-06)
    let board_signals_ctx: Option<BoardSignals> = use_context::<BoardSignals>();
    let card_id_for_flash = card.get_untracked().id.clone();
    let card_id_for_fading = card_id_for_flash.clone();

    view! {
        <div
            class="lns-card"
            class:lns-card--dragging=move || {
                drag_info.get().map_or(false, |d| d.is_dragging && d.card_id == card.get_untracked().id)
            }
            class:lns-card--remote-flash=move || {
                board_signals_ctx
                    .map(|bs| bs.highlight_card_id.get().as_deref() == Some(card_id_for_flash.as_str()))
                    .unwrap_or(false)
            }
            class:lns-card--fading=move || {
                board_signals_ctx
                    .map(|bs| bs.fading_card_ids.with(|fids| fids.contains(card_id_for_fading.as_str())))
                    .unwrap_or(false)
            }
            data-card-id=move || card.get_untracked().id.clone()
            data-list-id=list_id_clone.clone()
            on:click=move |_ev| {
                // Only open the modal if no drag occurred (guard: is_dragging must be false).
                // A drag-release fires a click event on the source element — we suppress it here.
                let is_dragging = drag_info.get().map_or(false, |d| d.is_dragging);
                if !is_dragging {
                    let cn = card.get_untracked().card_num;
                    let bid = board_id_for_click.clone();
                    let path = format!("/board/{}/card/{}", bid, cn);
                    #[cfg(target_arch = "wasm32")]
                    {
                        use leptos_router::hooks::use_navigate;
                        let navigate = use_navigate();
                        navigate(&path, Default::default());
                    }
                }
            }
            on:pointerdown=move |ev: leptos::ev::PointerEvent| {
                ev.prevent_default();
                let card_id = card.get_untracked().id.clone();
                let pointer_id = ev.pointer_id();
                #[cfg(target_arch = "wasm32")]
                {
                    use wasm_bindgen::JsCast;
                    if let Some(el) = ev.target()
                        .and_then(|t| t.dyn_into::<leptos::web_sys::Element>().ok())
                    {
                        let _ = el.set_pointer_capture(pointer_id);
                    }
                }
                drag_info.set(Some(DragInfo {
                    card_id,
                    from_list_id: list_id_for_drag.clone(),
                    pointer_id,
                    start_x: ev.client_x() as f64,
                    start_y: ev.client_y() as f64,
                    current_x: ev.client_x() as f64,
                    current_y: ev.client_y() as f64,
                    is_dragging: false,
                }));
            }
        >
            // ── Cover band ─────────────────────────────────────────────────
            <Show when=move || card.get().cover.is_some()>
                {move || {
                    let cover = card.get().cover.unwrap_or_default();
                    let validated = safe_cover_color(&cover).to_string();
                    view! {
                        <div
                            class="lns-card-cover"
                            style=format!("background:{}", validated)
                        />
                    }
                }}
            </Show>

            // ── Card body ────────────────────────────────────────────────
            <div class="lns-card-body">

                // ── Labels row ──────────────────────────────────────────
                <Show when=move || !card.get().labels.is_empty()>
                    <div class="lns-card-labels">
                        {move || {
                            card.get().labels.into_iter().map(|label| {
                                view! {
                                    <LabelChip label=label expanded=labels_expanded/>
                                }
                            }).collect_view()
                        }}
                    </div>
                </Show>

                // ── Title ───────────────────────────────────────────────
                <p
                    class="lns-card-title"
                    class:lns-card-title--done=move || card.get().done
                >
                    {move || card.get().title}
                </p>

                // ── Meta row ────────────────────────────────────────────
                <div class="lns-card-meta">

                    // Slot 1: Priority pill (hidden when P3 or done)
                    <Show when=move || {
                        let c = card.get();
                        !c.done && matches!(c.priority.as_deref(), Some("P1") | Some("P2"))
                    }>
                        {move || {
                            let prio = card.get().priority.unwrap_or_default();
                            let cls = match prio.as_str() {
                                "P1" => "lns-card-prio p1",
                                "P2" => "lns-card-prio p2",
                                _ => "lns-card-prio",
                            };
                            view! {
                                <span class=cls>{prio}</span>
                            }
                        }}
                    </Show>

                    // Slot 2: Due chip (hidden when done)
                    <Show when=move || {
                        let c = card.get();
                        !c.done && c.due_at.is_some()
                    }>
                        {move || {
                            // Bind due_at once; if a concurrent mutation cleared it,
                            // render nothing rather than falling back to epoch 0 which
                            // would show a misleading "Jan 1" overdue label (WR-07).
                            let Some(due_ms) = card.get().due_at else {
                                return ().into_any();
                            };
                            let (label, tone) = format_due(due_ms);
                            let cls = if tone.is_empty() {
                                "lns-card-meta-item".to_string()
                            } else {
                                format!("lns-card-meta-item {}", tone)
                            };
                            view! {
                                <span class=cls>
                                    <crate::components::icon::Icon name="clock"/>
                                    {label}
                                </span>
                            }.into_any()
                        }}
                    </Show>

                    // Slot 3: Done badge (only when done)
                    <Show when=move || card.get().done>
                        <span class="lns-card-meta-item done">
                            <crate::components::icon::Icon name="check"/>
                            "Done"
                        </span>
                    </Show>

                    // Slot 4: Checklist count (when checklist_total > 0)
                    <Show when=move || { card.get().checklist_total > 0 }>
                        {move || {
                            let c = card.get();
                            let cls = if c.checklist_done == c.checklist_total && c.checklist_total > 0 {
                                "lns-card-meta-item lns-card-meta-checklist lns-card-meta-checklist--done"
                            } else {
                                "lns-card-meta-item lns-card-meta-checklist"
                            };
                            view! {
                                <span class=cls>
                                    <crate::components::icon::Icon name="check"/>
                                    {format!("{}/{}", c.checklist_done, c.checklist_total)}
                                </span>
                            }
                        }}
                    </Show>

                    // Slot 5: Comment count (when > 0)
                    <Show when=move || { card.get().comment_count > 0 }>
                        {move || {
                            let count = card.get().comment_count;
                            view! {
                                <span class="lns-card-meta-item">
                                    <crate::components::icon::Icon name="chat"/>
                                    {count.to_string()}
                                </span>
                            }
                        }}
                    </Show>

                    // Slot 6: Attachment count (when > 0)
                    <Show when=move || { card.get().attachment_count > 0 }>
                        {move || {
                            let count = card.get().attachment_count;
                            view! {
                                <span class="lns-card-meta-item">
                                    <crate::components::icon::Icon name="paperclip"/>
                                    {count.to_string()}
                                </span>
                            }
                        }}
                    </Show>

                    // Slot 7: Avatar stack (pushed right, shown when member_ids non-empty)
                    <Show when=move || !card.get().member_ids.is_empty()>
                        {move || {
                            let ids = card.get().member_ids;
                            view! {
                                <div class="lns-avatar-stack" style="margin-left: auto">
                                    {ids.into_iter().map(|_id| {
                                        // Render avatar placeholder circle (Phase 5 will wire actual avatars)
                                        view! {
                                            <div class="lns-avatar lns-avatar--sm"/>
                                        }
                                    }).collect_view()}
                                </div>
                            }
                        }}
                    </Show>

                </div>
            </div>

            // ── Editing badge (06-04, D-10) ──────────────────────────────────
            // Absolute overlay at bottom-left; appears when another user has this card open.
            {
                let card_id_for_badge = card.get_untracked().id.clone();
                move || {
                    if let Some(bs) = board_signals_ctx {
                        let editors = bs.editing_card_ids.with(|m| {
                            m.get(&card_id_for_badge).cloned().unwrap_or_default()
                        });
                        if !editors.is_empty() {
                            let count = editors.len();
                            let label = if count == 1 {
                                "Editing".to_string()
                            } else {
                                format!("Editing ({count})")
                            };
                            return Some(view! {
                                <div class="lns-editing-badge">
                                    // Pencil icon (12px inline SVG)
                                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                                        <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/>
                                        <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/>
                                    </svg>
                                    <span>{label}</span>
                                </div>
                            }.into_any());
                        }
                    }
                    None
                }
            }
        </div>
    }
}
