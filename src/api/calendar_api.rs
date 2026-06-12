use leptos::prelude::*;
use crate::models::CalendarCard;

// ---------------------------------------------------------------------------
// Month math helpers (SSR-only)
// ---------------------------------------------------------------------------

/// Return `(start_ms, end_ms)` epoch milliseconds (UTC) for the given calendar month.
///
/// `start_ms` = 00:00:00 UTC on the first of the month.
/// `end_ms`   = 23:59:59 UTC on the last of the month.
///
/// Handles all edge cases correctly via chrono:
/// - Leap-year February (e.g. Feb 2024 → last = 29th)
/// - December → January year wrap (month 12 → next year month 1)
///
/// Panics: only if `year`/`month` are outside the valid range (e.g. month = 0 or 13),
/// which the caller controls.
#[cfg(feature = "ssr")]
pub fn month_bounds_ms(year: i32, month: u32) -> (i64, i64) {
    use chrono::{Duration, NaiveDate, NaiveTime, TimeZone, Utc};

    let first_day = NaiveDate::from_ymd_opt(year, month, 1)
        .expect("invalid year/month for month_bounds_ms");

    // Last day: first day of next month minus one day (handles Dec→Jan and leap years).
    let next_month_first = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .expect("overflow in month_bounds_ms next-month calculation");

    let last_day = next_month_first - Duration::days(1);

    let start_time = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
    let end_time = NaiveTime::from_hms_opt(23, 59, 59).unwrap();

    let start_ms = Utc
        .from_utc_datetime(&first_day.and_time(start_time))
        .timestamp_millis();

    let end_ms = Utc
        .from_utc_datetime(&last_day.and_time(end_time))
        .timestamp_millis();

    (start_ms, end_ms)
}

/// Compute the number of leading pad days to render before the 1st of the month
/// so the calendar grid starts on Monday (D-14, UI-SPEC).
///
/// Monday = 0, Tuesday = 1, ..., Sunday = 6
/// e.g. if 1st is Wednesday → 2 pad days (Mon + Tue)
#[cfg(feature = "ssr")]
pub fn leading_days(year: i32, month: u32) -> u32 {
    use chrono::{Datelike, NaiveDate};
    let first = NaiveDate::from_ymd_opt(year, month, 1)
        .expect("invalid year/month for leading_days");
    first.weekday().num_days_from_monday()
}

// ---------------------------------------------------------------------------
// Inner fn (testable, no Leptos context)
// ---------------------------------------------------------------------------

/// Fetch all due-date-bearing, non-archived cards for the given month bounds from all
/// boards the user is a member of.
///
/// Security (T-07-10): JOIN on board_members WHERE bm.user_id = ? ensures cross-board
/// isolation — a user sees only cards on boards they belong to.
///
/// The query does NOT include `AND c.due_at IS NOT NULL` explicitly because the
/// `AND c.due_at >= ?` already excludes NULLs (NULL comparisons are false in SQL).
#[cfg(feature = "ssr")]
pub async fn get_calendar_cards_inner(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<CalendarCard>, sqlx::Error> {
    sqlx::query_as!(
        CalendarCard,
        r#"SELECT c.id as "id!",
                  c.title as "title!",
                  c.card_num as "card_num!",
                  c.due_at,
                  CAST(c.done AS BOOLEAN) as "done!: bool",
                  c.board_id as "board_id!",
                  b.name as "board_name!",
                  b.color as "board_color!"
           FROM cards c
           JOIN boards b ON c.board_id = b.id
           JOIN board_members bm ON bm.board_id = c.board_id
           WHERE bm.user_id = ? AND c.archived = 0
             AND c.due_at >= ? AND c.due_at <= ?
           ORDER BY c.due_at ASC"#,
        user_id,
        start_ms,
        end_ms
    )
    .fetch_all(pool)
    .await
}

// ---------------------------------------------------------------------------
// Server fn wrapper
// ---------------------------------------------------------------------------

/// Fetch all due-date-bearing cards for the given (year, month) across all the
/// authenticated user's boards (D-11).
///
/// Returns `Vec<CalendarCard>` with cards ordered by due_at ASC.
///
/// Auth: requires a valid session (require_user); unauthenticated → ServerFnError.
/// Cross-board isolation: enforced by the JOIN on board_members (T-07-10).
#[server]
pub async fn get_calendar_cards(
    year: i32,
    month: u32,
) -> Result<Vec<CalendarCard>, ServerFnError> {
    use crate::auth::helpers::require_user;
    use crate::server::state::AppState;

    let state = expect_context::<AppState>();
    let user = require_user().await?;

    let (start_ms, end_ms) = month_bounds_ms(year, month);

    get_calendar_cards_inner(&state.read_pool.0, &user.id, start_ms, end_ms)
        .await
        .map_err(|e| {
            tracing::error!("get_calendar_cards error: {e}");
            ServerFnError::new("Failed to load calendar cards")
        })
}
