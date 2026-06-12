//! Inbox page — INBOX-01, INBOX-02.
//!
//! Lists the current user's notifications in time buckets (Today / Earlier this week / Older),
//! with unread/read styling (D-08), click-to-open-card + auto-mark-read (D-06),
//! per-item read toggle (INBOX-02), mark-all-read action (INBOX-02), and empty state (D-09).
//!
//! Auth guard: redirects to /login when unauthenticated.
//!
//! Threat mitigations:
//! - T-07-07: list_notifications scoped to current user via require_user
//! - T-07-06: mark_notification_read / mark_all_notifications_read user-scoped on server
//! - T-07-08: summary text rendered via Leptos view! (no inner_html; escaping by default)

use leptos::prelude::*;
use leptos_router::components::Redirect;
use leptos_router::hooks::use_navigate;
use crate::api::auth_api::get_current_user;
use crate::api::notification_api::{list_notifications, mark_notification_read, mark_all_notifications_read};
use crate::components::sidebar::WorkspaceSidebar;
use crate::components::icon::Icon;
use crate::models::NotificationRow;
use crate::state::ws_client::spawn_notif_task;

// ---------------------------------------------------------------------------
// Time-bucket helpers
// ---------------------------------------------------------------------------

/// Epoch millis for the start of today in UTC (midnight UTC of current date).
fn today_start_ms() -> i64 {
    #[cfg(feature = "ssr")]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        // Truncate to day boundary: floor to 86400000 ms
        (now / 86_400_000) * 86_400_000
    }
    #[cfg(all(not(feature = "ssr"), feature = "hydrate"))]
    {
        let now = js_sys::Date::now() as i64;
        (now / 86_400_000) * 86_400_000
    }
    #[cfg(all(not(feature = "ssr"), not(feature = "hydrate")))]
    {
        0
    }
}

/// Bucket label for a notification row based on its `created_at` epoch millis.
fn bucket_for(created_at: i64, today_start: i64) -> &'static str {
    let week_start = today_start - 6 * 86_400_000; // 7 days ago (Mon–Sun span ≈ 6 days before today)
    if created_at >= today_start {
        "Today"
    } else if created_at >= week_start {
        "Earlier this week"
    } else {
        "Older"
    }
}

/// Format epoch millis as a relative "N units ago" string.
/// Mirrors the relative_time helper in card_detail/mod.rs.
fn relative_time_inbox(created_at_ms: i64) -> String {
    let now_ms: i64;
    #[cfg(feature = "ssr")]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
    }
    #[cfg(all(not(feature = "ssr"), feature = "hydrate"))]
    {
        now_ms = js_sys::Date::now() as i64;
    }
    #[cfg(all(not(feature = "ssr"), not(feature = "hydrate")))]
    {
        now_ms = 0;
    }

    let diff_secs = ((now_ms - created_at_ms) / 1000).max(0);
    let diff_mins = diff_secs / 60;
    let diff_hours = diff_mins / 60;
    let diff_days = diff_hours / 24;
    let diff_weeks = diff_days / 7;
    let diff_months = diff_days / 30;

    if diff_secs < 60 {
        "just now".to_string()
    } else if diff_mins < 60 {
        if diff_mins == 1 { "1 minute ago".to_string() } else { format!("{} minutes ago", diff_mins) }
    } else if diff_hours < 24 {
        if diff_hours == 1 { "1 hour ago".to_string() } else { format!("{} hours ago", diff_hours) }
    } else if diff_days < 7 {
        if diff_days == 1 { "1 day ago".to_string() } else { format!("{} days ago", diff_days) }
    } else if diff_weeks < 5 {
        if diff_weeks == 1 { "1 week ago".to_string() } else { format!("{} weeks ago", diff_weeks) }
    } else if diff_months < 12 {
        if diff_months == 1 { "1 month ago".to_string() } else { format!("{} months ago", diff_months) }
    } else {
        "over a year ago".to_string()
    }
}

/// Build the one-line notification summary string (D-09 copywriting contract).
fn notification_summary(row: &NotificationRow) -> String {
    let actor = row.actor_name.as_deref().unwrap_or("Someone");
    let card = row.card_title.as_deref().unwrap_or("a card");
    match row.kind.as_str() {
        "mention" => format!("{} mentioned you on \"{}\"", actor, card),
        "assigned" => format!("{} assigned you to \"{}\"", actor, card),
        "due_soon" => format!("\"{}\" is due soon", card),
        "overdue" => format!("\"{}\" is overdue", card),
        "watch_activity" => format!("{} updated \"{}\"", actor, card),
        _ => format!("Notification on \"{}\"", card),
    }
}

// ---------------------------------------------------------------------------
// Auth guard shell
// ---------------------------------------------------------------------------

/// Inbox page — auth guard, then renders InboxContent.
#[component]
pub fn InboxPage() -> impl IntoView {
    let current_user = Resource::new(|| (), |_| async { get_current_user().await });

    view! {
        <Suspense fallback=|| ()>
            {move || current_user.get().map(|result| match result {
                Ok(None) => view! { <Redirect path="/login"/> }.into_any(),
                Err(_) => view! {
                    <p class="board-error">"Something went wrong determining your session."</p>
                }.into_any(),
                Ok(Some(_user)) => view! {
                    <InboxContent />
                }.into_any(),
            })}
        </Suspense>
    }
}

// ---------------------------------------------------------------------------
// Main inbox content
// ---------------------------------------------------------------------------

#[component]
fn InboxContent() -> impl IntoView {
    // ── Notification badge (live) ─────────────────────────────────────────
    let unread_count = RwSignal::new(0i64);
    let badge_pulse = RwSignal::new(false);
    {
        let notif_handle = StoredValue::new(Some(spawn_notif_task(unread_count, badge_pulse)));
        on_cleanup(move || {
            notif_handle.update_value(|h| { h.take(); });
        });
    }

    // ── Notifications resource ────────────────────────────────────────────
    let notifications = Resource::new(|| (), |_| async { list_notifications(50, 0).await });

    // ── Mark-all action ───────────────────────────────────────────────────
    let on_mark_all = move |_: leptos::ev::MouseEvent| {
        let notifs = notifications;
        leptos::task::spawn_local(async move {
            let _ = mark_all_notifications_read().await;
            notifs.refetch();
            unread_count.set(0);
        });
    };

    view! {
        <div class="lns-app">
            // Sidebar (no board data on inbox page — pass empty defaults)
            <WorkspaceSidebar
                all_boards=Signal::derive(|| vec![])
                starred_boards=Signal::derive(|| vec![])
                on_star=Callback::new(|_: String| {})
                unread_count=unread_count
                badge_pulse=badge_pulse
            />

            // Main column
            <div class="lns-app-main">
                // Top bar (simple inline header for inbox)
                <div class="lns-topbar lns-inbox-topbar">
                    <h1 class="lns-inbox-page-title">"Inbox"</h1>
                    <button
                        type="button"
                        class="lns-btn lns-btn--ghost lns-btn--sm"
                        on:click=on_mark_all
                    >
                        "Mark all read"
                    </button>
                </div>

                // Notifications list
                <div class="lns-inbox">
                    <Suspense fallback=move || view! {
                        <div class="lns-inbox-loading">
                            <div class="lns-inbox-skeleton"/>
                            <div class="lns-inbox-skeleton"/>
                            <div class="lns-inbox-skeleton"/>
                        </div>
                    }>
                        {move || notifications.get().map(|result| {
                            match result {
                                Err(_) => view! {
                                    <p class="lns-inbox-error">
                                        "Couldn't load notifications. Refresh to try again."
                                    </p>
                                }.into_any(),
                                Ok(rows) if rows.is_empty() => view! {
                                    <InboxEmptyState/>
                                }.into_any(),
                                Ok(rows) => view! {
                                    <InboxBuckets rows=rows notifications=notifications unread_count=unread_count />
                                }.into_any(),
                            }
                        })}
                    </Suspense>
                </div>
            </div>
        </div>
    }
}


// ---------------------------------------------------------------------------
// Bucket renderer
// ---------------------------------------------------------------------------

#[component]
fn InboxBuckets(
    rows: Vec<NotificationRow>,
    notifications: Resource<Result<Vec<NotificationRow>, ServerFnError>>,
    unread_count: RwSignal<i64>,
) -> impl IntoView {
    let today_start = today_start_ms();

    // Group into ordered buckets
    let buckets: Vec<(&'static str, Vec<NotificationRow>)> = {
        let mut today_rows: Vec<NotificationRow> = Vec::new();
        let mut week_rows: Vec<NotificationRow> = Vec::new();
        let mut older_rows: Vec<NotificationRow> = Vec::new();
        for row in rows {
            match bucket_for(row.created_at, today_start) {
                "Today" => today_rows.push(row),
                "Earlier this week" => week_rows.push(row),
                _ => older_rows.push(row),
            }
        }
        let mut result = Vec::new();
        if !today_rows.is_empty() { result.push(("Today", today_rows)); }
        if !week_rows.is_empty() { result.push(("Earlier this week", week_rows)); }
        if !older_rows.is_empty() { result.push(("Older", older_rows)); }
        result
    };

    view! {
        {buckets.into_iter().map(|(label, bucket_rows)| {
            view! {
                <div class="lns-inbox-bucket">
                    <div class="lns-inbox-bucket-label">{label}</div>
                    <div class="lns-inbox-list">
                        {bucket_rows.into_iter().map(|row| {
                            let notifications_ref = notifications;
                            let unread_count_ref = unread_count;
                            view! {
                                <InboxRow
                                    row=row
                                    notifications=notifications_ref
                                    unread_count=unread_count_ref
                                />
                            }
                        }).collect_view()}
                    </div>
                </div>
            }
        }).collect_view()}
    }
}

// ---------------------------------------------------------------------------
// Single row
// ---------------------------------------------------------------------------

#[component]
fn InboxRow(
    row: NotificationRow,
    notifications: Resource<Result<Vec<NotificationRow>, ServerFnError>>,
    unread_count: RwSignal<i64>,
) -> impl IntoView {
    let is_read = RwSignal::new(row.read);
    let notif_id = row.id.clone();
    let board_id = row.board_id.clone();
    let card_num = row.card_num;

    // Summary text and meta
    let summary = notification_summary(&row);
    let board_name = row.board_name.clone().unwrap_or_default();
    let rel_time = relative_time_inbox(row.created_at);
    let meta_line = format!("{} · {}", board_name, rel_time);

    // Icon name and CSS class per kind
    let (icon_name, icon_tile_class) = match row.kind.as_str() {
        "mention"        => ("chat",    "lns-inbox-row-icon lns-inbox-row-icon--mention"),
        "assigned"       => ("user",    "lns-inbox-row-icon lns-inbox-row-icon--assigned"),
        "due_soon"       => ("clock",   "lns-inbox-row-icon lns-inbox-row-icon--due-soon"),
        "overdue"        => ("clock",   "lns-inbox-row-icon lns-inbox-row-icon--overdue"),
        "watch_activity" => ("eye",     "lns-inbox-row-icon lns-inbox-row-icon--watch"),
        _                => ("bell",    "lns-inbox-row-icon"),
    };

    // Navigate to the deep-link on click (D-06) — only if we have board+card data
    let navigate = use_navigate();
    let on_row_click = {
        let notif_id_click = notif_id.clone();
        let notifications_click = notifications;
        move |_: leptos::ev::MouseEvent| {
            let id = notif_id_click.clone();
            let notifs = notifications_click;
            let was_read = is_read.get_untracked();
            leptos::task::spawn_local(async move {
                // Optimistically mark as read
                if !was_read {
                    is_read.set(true);
                    let _ = mark_notification_read(id).await;
                    notifs.refetch();
                    // Decrement badge (best-effort; refetch will correct it)
                    unread_count.update(|c| *c = (*c - 1).max(0));
                }
            });
            // Navigate to card deep-link if available (D-06)
            if let (Some(bid), Some(cnum)) = (board_id.as_deref(), card_num) {
                let href = format!("/board/{}/card/{}", bid, cnum);
                navigate(&href, Default::default());
            }
        }
    };

    // Per-item read toggle (hover action, D-06)
    let on_toggle_read = {
        let notif_id_toggle = notif_id.clone();
        let notifications_toggle = notifications;
        move |ev: leptos::ev::MouseEvent| {
            ev.stop_propagation();
            let id = notif_id_toggle.clone();
            let notifs = notifications_toggle;
            let was_read = is_read.get_untracked();
            leptos::task::spawn_local(async move {
                if !was_read {
                    is_read.set(true);
                    let _ = mark_notification_read(id).await;
                    notifs.refetch();
                    unread_count.update(|c| *c = (*c - 1).max(0));
                }
            });
        }
    };

    view! {
        <div
            class=move || if is_read.get() { "lns-inbox-row lns-inbox-row--read" } else { "lns-inbox-row lns-inbox-row--unread" }
            on:click=on_row_click
        >
            // Unread dot (absolutely positioned, only for unread rows)
            <Show when=move || !is_read.get()>
                <span class="lns-inbox-row-dot" aria-hidden="true"/>
            </Show>

            // Kind icon tile
            <div class=icon_tile_class>
                <Icon name=icon_name/>
            </div>

            // Summary + meta
            <div class="lns-inbox-row-body">
                <div class="lns-inbox-row-summary">{summary}</div>
                <div class="lns-inbox-row-meta">{meta_line}</div>
            </div>

            // Hover action: read toggle
            <div class="lns-inbox-row-actions">
                <button
                    type="button"
                    class="lns-inbox-row-toggle"
                    aria-label=move || if is_read.get() { "Mark as unread" } else { "Mark as read" }
                    on:click=on_toggle_read
                >
                    <Icon name="eye"/>
                </button>
            </div>
        </div>
    }
}

// ---------------------------------------------------------------------------
// Empty state (D-09)
// ---------------------------------------------------------------------------

#[component]
fn InboxEmptyState() -> impl IntoView {
    view! {
        <div class="lns-inbox-empty">
            <div class="lns-inbox-empty-icon">
                <Icon name="sparkle"/>
            </div>
            <h2 class="lns-inbox-empty-heading">"You're all caught up"</h2>
            <p class="lns-inbox-empty-body">
                "New mentions, assignments, and due-date reminders will appear here."
            </p>
        </div>
    }
}
