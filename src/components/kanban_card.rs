use leptos::prelude::*;
use crate::models::Card;
use crate::components::label_chip::LabelChip;

/// Validate a CSS color string for safe interpolation into inline styles (T-04-06).
/// Accepts #rrggbb, #rgb, and oklch(...) shapes. Falls back to transparent/neutral.
fn safe_cover_color(c: &str) -> &str {
    let s = c.trim();
    if s.starts_with("oklch(") && s.ends_with(')') {
        return c;
    }
    if s.starts_with('#') && (s.len() == 7 || s.len() == 4)
        && s[1..].chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return c;
    }
    "transparent"
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
    use std::time::{SystemTime, UNIX_EPOCH};

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

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

    // Format as "Mon DD" using simple epoch math
    // We use chrono-free formatting: just compute day/month from millis
    let secs = due_at_ms / 1000;
    let days_since_epoch = secs / 86400;

    // Simple Gregorian calendar calculation
    let label = epoch_days_to_mon_day(days_since_epoch as u64);

    (label, tone)
}

/// Convert days since Unix epoch (1970-01-01) to "Mon DD" format (e.g. "May 20").
fn epoch_days_to_mon_day(days: u64) -> String {
    // Algorithm: compute year/month/day from Julian day number
    // Days since epoch = days since 1970-01-01
    let jdn = days + 2440588; // Julian Day Number offset from 1970-01-01

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

    let month_name = if month as usize <= 12 { MONTHS[month as usize] } else { "?" };
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
/// Drag/pointer handlers are deferred to Plan 03 (additive signature extension point).
#[component]
pub fn KanbanCard(
    card: RwSignal<Card>,
    labels_expanded: RwSignal<bool>,
    list_id: String,
) -> impl IntoView {
    let list_id_clone = list_id.clone();

    view! {
        <div
            class="lns-card"
            attr:data-card-id=move || card.get_untracked().id.clone()
            attr:data-list-id=list_id_clone.clone()
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
                            let due_ms = card.get().due_at.unwrap_or(0);
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
                            }
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
        </div>
    }
}
