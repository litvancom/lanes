//! Calendar page — CAL-01.
//!
//! Month-grid view of due-date-bearing cards across all the user's boards (D-11).
//! Monday-start week layout (D-14), prev/next/Today month navigation (D-12),
//! board-colored chips with overdue/done styling (D-14), "+N more" inline expansion (D-12),
//! chip click navigates to `/board/{board_id}/card/{card_num}` (D-13).
//!
//! Auth guard: redirects to /login when unauthenticated.
//!
//! Threat mitigations:
//! - T-07-10: get_calendar_cards JOINs board_members on require_user (cross-board isolation)
//! - T-07-11: safe_hex() validates board color before CSS interpolation into --chip-color
//! - T-07-12: card title rendered as Leptos text node (no inner_html; auto-escaped)

use leptos::prelude::*;
use leptos_router::components::Redirect;
use leptos_router::hooks::use_navigate;
use crate::api::auth_api::get_current_user;
use crate::api::calendar_api::get_calendar_cards;
use crate::components::sidebar::WorkspaceSidebar;
use crate::components::icon::Icon;
use crate::components::board_card::safe_hex;
use crate::models::CalendarCard;
use crate::state::ws_client::spawn_notif_task;

// ---------------------------------------------------------------------------
// Current month helper (SSR + WASM safe, Pitfall 4)
// ---------------------------------------------------------------------------

/// Return the current `(year, month)` from a platform-appropriate clock.
///
/// SSR: uses `SystemTime` → chrono UTC for stable, fresh current time.
/// Hydrate (WASM): uses `js_sys::Date::now()` via epoch millis → chrono.
/// Fallback: returns (2026, 6) as a safe default if neither feature is active.
///
/// Pitfall 4: initializing from a stable UTC epoch avoids a midnight-crossing
/// SSR/hydration mismatch on the month boundary.
fn current_year_month() -> (i32, u32) {
    #[cfg(feature = "ssr")]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        use chrono::{Datelike, DateTime, Utc};
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let dt = DateTime::<Utc>::from_timestamp_millis(ms).unwrap_or_default();
        (dt.year(), dt.month())
    }
    #[cfg(all(not(feature = "ssr"), feature = "hydrate"))]
    {
        use chrono::{Datelike, DateTime, Utc};
        let ms = js_sys::Date::now() as i64;
        let dt = DateTime::<Utc>::from_timestamp_millis(ms).unwrap_or_default();
        (dt.year(), dt.month())
    }
    #[cfg(all(not(feature = "ssr"), not(feature = "hydrate")))]
    {
        (2026, 6)
    }
}

/// Return epoch millis for "now" — used for overdue/due-soon chip classification.
fn now_ms() -> i64 {
    #[cfg(feature = "ssr")]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
    #[cfg(all(not(feature = "ssr"), feature = "hydrate"))]
    {
        js_sys::Date::now() as i64
    }
    #[cfg(all(not(feature = "ssr"), not(feature = "hydrate")))]
    {
        0
    }
}

// ---------------------------------------------------------------------------
// Month math (client-side grid layout, mirrors server-side month_bounds_ms)
// ---------------------------------------------------------------------------

/// Month name array (0-indexed; use `month - 1` to index).
const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
];

/// Number of days in a given month (handles leap years via simple rule).
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            // Leap year: divisible by 4, except centuries not divisible by 400
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Compute leading pad days for Monday-start grid.
///
/// Result is the weekday index of the 1st, Monday=0..Sunday=6.
/// Uses the same Tomohiko Sakamoto-derived formula used by chrono internally
/// so results match `leading_days()` in `calendar_api.rs` (D-14).
fn leading_pad_days(year: i32, month: u32) -> u32 {
    // Zeller-style: weekday of day 1 of the month, 0=Sun..6=Sat → remap to Mon=0..Sun=6
    // We use the fact that Jan 1 2001 was a Monday (weekday 1 in Sun=0 convention).
    // Instead, use the simple t-array formula (Tomohiko Sakamoto).
    let t: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year } as u64;
    let m = month as u64;
    let d: u64 = 1;
    // Sunday = 0
    let dow_sun0 = (y + y / 4 - y / 100 + y / 400 + t[(m as usize) - 1] as u64 + d) % 7;
    // Convert Sun=0..Sat=6 to Mon=0..Sun=6
    if dow_sun0 == 0 { 6 } else { (dow_sun0 - 1) as u32 }
}

// ---------------------------------------------------------------------------
// Auth guard shell
// ---------------------------------------------------------------------------

/// Calendar page — auth guard, then renders CalendarContent.
#[component]
pub fn CalendarPage() -> impl IntoView {
    let current_user = Resource::new(|| (), |_| async { get_current_user().await });

    view! {
        <Suspense fallback=|| ()>
            {move || current_user.get().map(|result| match result {
                Ok(None) => view! { <Redirect path="/login"/> }.into_any(),
                Err(_) => view! {
                    <p class="board-error">"Something went wrong determining your session."</p>
                }.into_any(),
                Ok(Some(_user)) => view! {
                    <CalendarContent />
                }.into_any(),
            })}
        </Suspense>
    }
}

// ---------------------------------------------------------------------------
// CalendarContent (main shell)
// ---------------------------------------------------------------------------

#[component]
fn CalendarContent() -> impl IntoView {
    // ── Notification badge (live) ─────────────────────────────────────────
    let unread_count = RwSignal::new(0i64);
    let badge_pulse = RwSignal::new(false);
    {
        let notif_handle = StoredValue::new(Some(spawn_notif_task(unread_count, badge_pulse)));
        on_cleanup(move || {
            notif_handle.update_value(|h| { h.take(); });
        });
    }

    // ── Month signal (default: current month) ────────────────────────────
    let displayed = RwSignal::new(current_year_month());

    // ── Calendar cards resource ───────────────────────────────────────────
    let cards = Resource::new(
        move || displayed.get(),
        |(y, m)| async move { get_calendar_cards(y, m).await },
    );

    view! {
        <div class="lns-app">
            // Sidebar
            <WorkspaceSidebar
                all_boards=Signal::derive(|| vec![])
                starred_boards=Signal::derive(|| vec![])
                on_star=Callback::new(|_: String| {})
                unread_count=unread_count
                badge_pulse=badge_pulse
            />

            // Main column
            <div class="lns-app-main">
                // Top bar
                <div class="lns-topbar lns-calendar-topbar">
                    <h1 class="lns-calendar-page-title">"Calendar"</h1>
                </div>

                // Calendar body
                <div class="lns-calendar">
                    <CalendarHeader displayed=displayed />

                    <Suspense fallback=move || view! {
                        <div class="lns-calendar-grid lns-calendar-grid--loading">
                            <span class="lns-loading-text">"Loading…"</span>
                        </div>
                    }>
                        {move || cards.get().map(|result| match result {
                            Ok(card_list) => {
                                let (year, month) = displayed.get();
                                view! {
                                    <CalendarGrid
                                        cards=card_list
                                        year=year
                                        month=month
                                    />
                                }.into_any()
                            }
                            Err(_) => view! {
                                <div class="lns-calendar-error">
                                    "Failed to load calendar. Refresh to try again."
                                </div>
                            }.into_any(),
                        })}
                    </Suspense>
                </div>
            </div>
        </div>
    }
}

// ---------------------------------------------------------------------------
// CalendarHeader — month nav (prev/next/Today)
// ---------------------------------------------------------------------------

#[component]
fn CalendarHeader(displayed: RwSignal<(i32, u32)>) -> impl IntoView {
    let (cur_year, cur_month) = current_year_month();

    let is_current_month = move || {
        let (y, m) = displayed.get();
        y == cur_year && m == cur_month
    };

    // Month label: "June 2026"
    let month_label = move || {
        let (y, m) = displayed.get();
        let name = MONTH_NAMES.get((m as usize).saturating_sub(1)).copied().unwrap_or("?");
        format!("{} {}", name, y)
    };

    // Prev month: Dec → prev year Jan
    let on_prev = move |_: leptos::ev::MouseEvent| {
        displayed.update(|(y, m)| {
            if *m == 1 {
                *m = 12;
                *y -= 1;
            } else {
                *m -= 1;
            }
        });
    };

    // Next month: Dec → next year Jan
    let on_next = move |_: leptos::ev::MouseEvent| {
        displayed.update(|(y, m)| {
            if *m == 12 {
                *m = 1;
                *y += 1;
            } else {
                *m += 1;
            }
        });
    };

    // Today: snap back to current month
    let on_today = move |_: leptos::ev::MouseEvent| {
        displayed.set(current_year_month());
    };

    view! {
        <div class="lns-calendar-header">
            // Prev: chevron rotated 180° (pointing left)
            <button
                type="button"
                class="lns-calendar-nav-btn lns-calendar-nav-btn--prev"
                aria-label="Previous month"
                on:click=on_prev
            >
                <Icon name="chevron"/>
            </button>

            <span class="lns-calendar-month-label">{month_label}</span>

            // Next: standard chevron (pointing right)
            <button
                type="button"
                class="lns-calendar-nav-btn lns-calendar-nav-btn--next"
                aria-label="Next month"
                on:click=on_next
            >
                <Icon name="chevron"/>
            </button>

            <button
                type="button"
                class="lns-btn lns-btn--ghost lns-btn--sm lns-calendar-today-btn"
                on:click=on_today
                disabled=is_current_month
            >
                "Today"
            </button>
        </div>
    }
}

// ---------------------------------------------------------------------------
// CalendarGrid — day-of-week header + day cells
// ---------------------------------------------------------------------------

#[component]
fn CalendarGrid(
    cards: Vec<CalendarCard>,
    year: i32,
    month: u32,
) -> impl IntoView {
    const DOW_LABELS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

    let leading = leading_pad_days(year, month);
    let total_days = days_in_month(year, month);
    let (cur_year, cur_month) = current_year_month();

    // Convert epoch millis of a due date to a day-of-month number (1-based) for the displayed month.
    // We use UTC epoch → day number via simple division (86400000ms per day) + chrono.
    // Because due_at is stored as UTC epoch millis we can recover the day in UTC.
    let day_of_month = move |due_ms: i64| -> Option<u32> {
        // Convert epoch millis to a UTC date and extract day-of-month for the displayed month.
        use chrono::{Datelike, DateTime, Utc};
        let dt = DateTime::<Utc>::from_timestamp_millis(due_ms)?;
        if dt.year() == year && dt.month() == month {
            Some(dt.day())
        } else {
            None
        }
    };

    // Group cards by day number
    let mut by_day: std::collections::HashMap<u32, Vec<CalendarCard>> = std::collections::HashMap::new();
    for card in &cards {
        if let Some(due_ms) = card.due_at {
            if let Some(day) = day_of_month(due_ms) {
                by_day.entry(day).or_default().push(card.clone());
            }
        }
    }

    // Determine today's day number (if in displayed month)
    let today_day: Option<u32> = if cur_year == year && cur_month == month {
        // Today's day number in the displayed month
        #[cfg(feature = "ssr")]
        {
            use std::time::{SystemTime, UNIX_EPOCH};
            use chrono::{Datelike, DateTime, Utc};
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let dt = DateTime::<Utc>::from_timestamp_millis(ms).unwrap_or_default();
            Some(dt.day())
        }
        #[cfg(not(feature = "ssr"))]
        {
            use chrono::{Datelike, DateTime, Utc};
            let ms = js_sys::Date::now() as i64;
            let dt = DateTime::<Utc>::from_timestamp_millis(ms).unwrap_or_default();
            Some(dt.day())
        }
    } else {
        None
    };

    // Build the day cells list: leading pads + month days
    let total_cells = (leading + total_days) as usize;

    view! {
        <div class="lns-calendar-grid">
            // Day-of-week header row
            {DOW_LABELS.iter().map(|&label| view! {
                <div class="lns-calendar-dow">{label}</div>
            }).collect::<Vec<_>>()}

            // Leading pad cells
            {(0..leading).map(|_| view! {
                <div class="lns-calendar-day lns-calendar-day--pad"></div>
            }).collect::<Vec<_>>()}

            // Month day cells
            {(1..=total_days).map(|day| {
                let is_today = today_day == Some(day);
                let day_cards = by_day.get(&day).cloned().unwrap_or_default();
                let overflow_count = if day_cards.len() > 3 {
                    day_cards.len() - 3
                } else {
                    0
                };

                view! {
                    <DayCell
                        day=day
                        is_today=is_today
                        cards=day_cards
                        overflow_count=overflow_count
                    />
                }
            }).collect::<Vec<_>>()}

            // Trailing pad cells to complete the final row (optional — keeps grid clean)
            {
                let trailing = if total_cells % 7 == 0 { 0 } else { 7 - (total_cells % 7) };
                (0..trailing).map(|_| view! {
                    <div class="lns-calendar-day lns-calendar-day--pad"></div>
                }).collect::<Vec<_>>()
            }
        </div>
    }
}

// ---------------------------------------------------------------------------
// DayCell — single day in the grid
// ---------------------------------------------------------------------------

#[component]
fn DayCell(
    day: u32,
    is_today: bool,
    cards: Vec<CalendarCard>,
    overflow_count: usize,
) -> impl IntoView {
    // Per-day expanded signal for "+N more" overflow (D-12)
    let expanded = RwSignal::new(false);

    // "Today" depends on the wall clock, which differs between the server (UTC)
    // and the browser → gate the highlight on hydration so SSR and the initial
    // client render match; the highlight fills in after mount (crate::hydration).
    let hydrated = crate::hydration::use_hydrated();
    let cell_class = move || {
        if is_today && hydrated.get() {
            "lns-calendar-day lns-calendar-day--today"
        } else {
            "lns-calendar-day"
        }
    };

    view! {
        <div class=cell_class>
            <div class="lns-calendar-day-num">{day}</div>

            <div class="lns-calendar-chips">
                // Always-visible chips (up to 3, or all when expanded)
                {cards.iter().enumerate().map(|(i, card)| {
                    let show = if overflow_count > 0 {
                        // Show first 3 always; show rest only when expanded
                        i < 3 || expanded.get()
                    } else {
                        true
                    };
                    let card = card.clone();
                    if show {
                        view! { <CalChip card=card /> }.into_any()
                    } else {
                        view! { <span style="display:none"/> }.into_any()
                    }
                }).collect::<Vec<_>>()}
            </div>

            // "+N more" / "Show less" toggle
            {if overflow_count > 0 {
                view! {
                    <button
                        type="button"
                        class="lns-calendar-overflow"
                        on:click=move |_| expanded.update(|e| *e = !*e)
                    >
                        {move || if expanded.get() {
                            "Show less".to_string()
                        } else {
                            format!("+{} more", overflow_count)
                        }}
                    </button>
                }.into_any()
            } else {
                view! { <span/> }.into_any()
            }}
        </div>
    }
}

// ---------------------------------------------------------------------------
// CalChip — a single card chip in a day cell
// ---------------------------------------------------------------------------

/// A board-colored card chip in a calendar day cell.
///
/// Styling (UI-SPEC chip design):
/// - `--chip-color` CSS custom property set to `safe_hex(board_color)`.
/// - Overdue (due_at < now && !done): `.due-overdue` class override.
/// - Due soon (due_at in next 24h && !done): `.due-soon` class override.
/// - Done: `.done` (opacity + line-through, no color override).
///
/// Click navigates to `/board/{board_id}/card/{card_num}` (D-13, T-07-12).
/// `aria-label` = "{title} · {board name} · due {date}" (UI-SPEC copywriting contract).
#[component]
fn CalChip(card: CalendarCard) -> impl IntoView {
    let navigate = use_navigate();
    let board_id = card.board_id.clone();
    let card_num = card.card_num;
    let board_color = safe_hex(&card.board_color).to_string();
    let due_ms = card.due_at;
    let done = card.done;

    // The overdue/due-soon tone is clock-derived → gate on hydration so SSR and
    // the initial client render emit the same class; the tone fills in after
    // mount (see crate::hydration). The `done` class is data-derived and stays.
    let hydrated = crate::hydration::use_hydrated();
    let chip_class = move || {
        let base = "lns-cal-chip";
        if done {
            format!("{} done", base)
        } else if hydrated.get() {
            if let Some(due) = due_ms {
                let now = now_ms();
                if due < now {
                    format!("{} due-overdue", base)
                } else if due <= now + 24 * 60 * 60 * 1000 {
                    format!("{} due-soon", base)
                } else {
                    base.to_string()
                }
            } else {
                base.to_string()
            }
        } else {
            base.to_string()
        }
    };

    // Aria label: "{title} · {board name} · due {date}"
    let aria = {
        let due_str = if let Some(ms) = due_ms {
            use chrono::{DateTime, Utc};
            DateTime::<Utc>::from_timestamp_millis(ms)
                .map(|dt| dt.format("%b %-d").to_string())
                .unwrap_or_else(|| "unknown date".to_string())
        } else {
            String::new()
        };
        format!("{} · {} · due {}", card.title, card.board_name, due_str)
    };

    // T-07-11: board color sanitized through safe_hex before CSS interpolation.
    let chip_style = format!("--chip-color: {}", board_color);
    let title = card.title.clone();

    let on_click = move |_: leptos::ev::MouseEvent| {
        let path = format!("/board/{}/card/{}", board_id, card_num);
        navigate(&path, Default::default());
    };

    view! {
        <button
            type="button"
            class=chip_class
            style=chip_style
            aria-label=aria
            on:click=on_click
        >
            {title}
        </button>
    }
}
