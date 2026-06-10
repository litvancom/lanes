//! Seed module: populates demo workspace fixtures into an empty SQLite database.
//!
//! Security (T-01-SC-SEED): all inserts use parameterized `sqlx::query(...).bind(...)`.
//! No `format!`-built SQL strings anywhere in this module.
//!
//! D-08: The non-empty guard runs first; if any users exist the function returns
//! `Err(SeedError::DatabaseNotEmpty)` and inserts nothing.
//!
//! D-09: Representative subset — one user "Mira", one board "Home & Life",
//! 4 lists, 8 cards with labels, checklist, comments, members, due dates,
//! priorities, and a Done card.
//!
//! All inserts are wrapped in a single transaction (T-02-02).

use fractional_index::FractionalIndex;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum SeedError {
    #[error("Database is non-empty. Seed refused. Drop and recreate the DB to reseed.")]
    DatabaseNotEmpty,
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),
}

/// Reset a user's password hash — CLI admin command (D-20).
///
/// Rejects passwords shorter than 8 characters (D-17).
/// Hashes the new password via spawn_blocking (Pitfall 9, T-02-10).
/// Returns Err if the user email is not found.
///
/// # Security
/// This function updates the `password_hash` column directly via a parameterized UPDATE.
/// The CLI caller is trusted (local machine, admin access to the binary) — T-02-11.
#[cfg(feature = "ssr")]
pub async fn reset_password(
    pool: &sqlx::SqlitePool,
    email: &str,
    new_password: &str,
) -> Result<(), String> {
    if new_password.len() < 8 {
        return Err("Password must be at least 8 characters.".to_string());
    }

    let new_password_owned = new_password.to_string();
    let hash = tokio::task::spawn_blocking(move || {
        password_auth::generate_hash(new_password_owned)
    })
    .await
    .map_err(|e| format!("Hash error: {e}"))?;

    let email_lower = email.trim().to_lowercase();

    let result = sqlx::query(
        "UPDATE users SET password_hash = ? WHERE email = ?",
    )
    .bind(&hash)
    .bind(&email_lower)
    .execute(pool)
    .await
    .map_err(|e| format!("Database error: {e}"))?;

    if result.rows_affected() == 0 {
        return Err(format!("No user found with email: {}", email_lower));
    }

    Ok(())
}

/// Seed the database with representative demo fixtures.
///
/// Refuses to run if any rows exist in the `users` table (D-08).
/// All inserts run in a single transaction (T-02-02).
pub async fn run_seed(write_pool: &sqlx::SqlitePool) -> Result<(), SeedError> {
    // D-08: Non-empty guard — check before opening a transaction
    let (user_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(write_pool)
        .await?;
    if user_count > 0 {
        return Err(SeedError::DatabaseNotEmpty);
    }

    // D-10: Hash Mira's demo password BEFORE the transaction — Argon2id is CPU-intensive
    // and must NOT block the async executor (Pitfall 9, T-02-10).
    let password_hash = tokio::task::spawn_blocking(|| {
        password_auth::generate_hash("lanes-demo")
    })
    .await
    .expect("password hash spawn_blocking");

    // All fixture inserts are inside a single transaction (T-02-02).
    let mut tx = write_pool.begin().await?;

    // ------------------------------------------------------------------
    // Timestamps — epoch millis UTC (D-03)
    // ------------------------------------------------------------------
    let now: i64 = crate::server::now_millis().expect("time went backwards");

    // ------------------------------------------------------------------
    // User: Mira (D-09, D-10)
    // Demo credential: mira@example.com / lanes-demo (documented, not a production secret — T-02-11)
    // ------------------------------------------------------------------
    let user_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, avatar_color, auth_provider, created_at) \
         VALUES (?, ?, ?, ?, ?, 'password', ?)",
    )
    .bind(&user_id)
    .bind("mira@example.com")
    .bind(&password_hash)
    .bind("Mira")
    .bind("#7c5cff")
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // ------------------------------------------------------------------
    // User: Alex (demo co-member, no password — password_hash nullable)
    // ------------------------------------------------------------------
    let alex_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, avatar_color, auth_provider, created_at) \
         VALUES (?, ?, NULL, ?, ?, 'password', ?)",
    )
    .bind(&alex_id)
    .bind("alex@example.com")
    .bind("Alex")
    .bind("#0ea5e9")
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // ------------------------------------------------------------------
    // User: Jamie (demo co-member, no password — password_hash nullable)
    // ------------------------------------------------------------------
    let jamie_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, avatar_color, auth_provider, created_at) \
         VALUES (?, ?, NULL, ?, ?, 'password', ?)",
    )
    .bind(&jamie_id)
    .bind("jamie@example.com")
    .bind("Jamie")
    .bind("#10b981")
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // ------------------------------------------------------------------
    // Board: "Home & Life" (D-09)
    // ------------------------------------------------------------------
    let board_id = Uuid::now_v7().to_string();
    // next_card_num will be set to seeded card count + 1 after inserts (updated below)
    sqlx::query(
        "INSERT INTO boards (id, name, key_prefix, next_card_num, color, starred, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(&board_id)
    .bind("Home & Life")
    .bind("HOME")
    .bind(1_i64) // placeholder; updated after cards are inserted
    .bind("#7c5cff")
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // board_members: owner row
    sqlx::query(
        "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, ?)",
    )
    .bind(&board_id)
    .bind(&user_id)
    .bind("owner")
    .execute(&mut *tx)
    .await?;

    // ------------------------------------------------------------------
    // Lists: Inbox / This week / In progress / Done  (D-09)
    // Positions generated with FractionalIndex::default() then new_after.
    // D-13: "Done" list has is_done_list = 1.
    // ------------------------------------------------------------------
    // (name, is_done_list)
    let list_defs: [(&str, i64); 4] = [
        ("Inbox",       0),
        ("This week",   0),
        ("In progress", 0),
        ("Done",        1),  // D-13: done-list flag
    ];
    let mut list_ids: Vec<String> = Vec::with_capacity(4);
    let mut prev_pos = FractionalIndex::default();

    for (i, (name, is_done_list)) in list_defs.iter().enumerate() {
        let list_id = Uuid::now_v7().to_string();
        let pos = if i == 0 {
            FractionalIndex::default()
        } else {
            FractionalIndex::new_after(&prev_pos)
        };
        let pos_str = pos.to_string();

        sqlx::query(
            "INSERT INTO lists (id, board_id, name, position, archived, is_done_list) VALUES (?, ?, ?, ?, 0, ?)",
        )
        .bind(&list_id)
        .bind(&board_id)
        .bind(name)
        .bind(&pos_str)
        .bind(is_done_list)
        .execute(&mut *tx)
        .await?;

        prev_pos = pos;
        list_ids.push(list_id);
    }

    // list_ids[0] = Inbox, [1] = This week, [2] = In progress, [3] = Done

    // ------------------------------------------------------------------
    // Labels (8) — board-scoped design-ref label set (D-09, UI-SPEC label table)
    // Colors are the resolved oklch values from --label-* CSS tokens.
    // ------------------------------------------------------------------
    // (name, color)
    let label_defs: [(&str, &str); 8] = [
        ("urgent",  "oklch(72% 0.10 25)"),
        ("errand",  "oklch(74% 0.10 60)"),
        ("health",  "oklch(72% 0.09 150)"),
        ("finance", "oklch(68% 0.10 295)"),
        ("family",  "oklch(74% 0.09 350)"),
        ("travel",  "oklch(70% 0.07 200)"),
        ("home",    "oklch(68% 0.10 240)"),
        ("someday", "oklch(72% 0.005 0)"),
    ];

    let mut label_ids: Vec<String> = Vec::with_capacity(8);
    for (name, color) in &label_defs {
        let lid = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO labels (id, board_id, name, color) VALUES (?, ?, ?, ?)",
        )
        .bind(&lid)
        .bind(&board_id)
        .bind(name)
        .bind(color)
        .execute(&mut *tx)
        .await?;
        label_ids.push(lid);
    }
    // label_ids indices: 0=urgent, 1=errand, 2=health, 3=finance, 4=family, 5=travel, 6=home, 7=someday
    let label_urgent_id  = &label_ids[0];
    let label_errand_id  = &label_ids[1];
    let label_finance_id = &label_ids[3];
    let label_family_id  = &label_ids[4];
    let label_travel_id  = &label_ids[5];
    let label_home_id    = &label_ids[6];

    // ------------------------------------------------------------------
    // Cards — 8 cards spread across lists, card_num 1-8 (D-02, D-09)
    // ------------------------------------------------------------------
    // Helper: generate sequential fractional positions per list
    // We'll track current last position per list index.
    let mut last_card_pos: [Option<FractionalIndex>; 4] = [None, None, None, None];

    /// Return the next card position for a given list index, advancing it.
    fn next_card_position(last: &mut Option<FractionalIndex>) -> FractionalIndex {
        let pos = match last {
            None => FractionalIndex::default(),
            Some(prev) => FractionalIndex::new_after(prev),
        };
        *last = Some(pos.clone());
        pos
    }

    // Card 1: Inbox — "Buy groceries", P3 (design-ref: home label)
    let card1_id = Uuid::now_v7().to_string();
    let pos1 = next_card_position(&mut last_card_pos[0]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, position, \
         priority, due_at, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(&card1_id)
    .bind(&list_ids[0])
    .bind(&board_id)
    .bind(1_i64)
    .bind("Buy groceries")
    .bind("Weekly grocery run")
    .bind(pos1.to_string())
    .bind("P3")
    .bind(now + 2 * 24 * 3600 * 1000_i64) // due in 2 days
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 2: Inbox — "Call plumber", P1, urgent label
    let card2_id = Uuid::now_v7().to_string();
    let pos2 = next_card_position(&mut last_card_pos[0]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, position, \
         priority, due_at, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(&card2_id)
    .bind(&list_ids[0])
    .bind(&board_id)
    .bind(2_i64)
    .bind("Call plumber")
    .bind("Leaky pipe under sink")
    .bind(pos2.to_string())
    .bind("P1")
    .bind(now + 1 * 24 * 3600 * 1000_i64) // due tomorrow
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 3: This week — "Prepare weekly review", P2
    let card3_id = Uuid::now_v7().to_string();
    let pos3 = next_card_position(&mut last_card_pos[1]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, position, \
         priority, due_at, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(&card3_id)
    .bind(&list_ids[1])
    .bind(&board_id)
    .bind(3_i64)
    .bind("Prepare weekly review")
    .bind(Option::<String>::None)
    .bind(pos3.to_string())
    .bind("P2")
    .bind(Option::<i64>::None)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 4: This week — "File expense reports", P1, finance label, 2 attachments
    // (design-ref c7: finance, P1, soon due, 2 attachments, me member)
    let card4_id = Uuid::now_v7().to_string();
    let pos4 = next_card_position(&mut last_card_pos[1]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, position, \
         priority, due_at, attachment_count, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(&card4_id)
    .bind(&list_ids[1])
    .bind(&board_id)
    .bind(4_i64)
    .bind("File expense reports")
    .bind("Q3 receipts")
    .bind(pos4.to_string())
    .bind("P1")
    .bind(now + 3 * 24 * 3600 * 1000_i64) // due soon
    .bind(2_i64) // attachment_count
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 5: In progress — "Fix garage door", P1, urgent+home labels, 1 comment, me member
    // (design-ref c9: urgent+home, P1, overdue/due today, 1 comment, me)
    let card5_id = Uuid::now_v7().to_string();
    let pos5 = next_card_position(&mut last_card_pos[2]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, position, \
         priority, due_at, comment_count, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(&card5_id)
    .bind(&list_ids[2])
    .bind(&board_id)
    .bind(5_i64)
    .bind("Fix garage door")
    .bind("Spring is broken")
    .bind(pos5.to_string())
    .bind("P1")
    .bind(now) // due today (overdue tone in UI)
    .bind(1_i64) // comment_count
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 6: In progress — "Plan holiday menu", P2, home+errand labels, cover, checklist 3/8, 1 comment, me+alex
    // (design-ref c6: cover #f5e6d3, home+errand, P2, soon due, 3/8 checklist, 1 comment, me+al)
    let card6_id = Uuid::now_v7().to_string();
    let pos6 = next_card_position(&mut last_card_pos[2]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, cover, position, \
         priority, due_at, checklist_done, checklist_total, comment_count, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(&card6_id)
    .bind(&list_ids[2])
    .bind(&board_id)
    .bind(6_i64)
    .bind("Plan holiday menu")
    .bind("Thanksgiving menu planning")
    .bind("#f5e6d3") // cover
    .bind(pos6.to_string())
    .bind("P2")
    .bind(now + 7 * 24 * 3600 * 1000_i64) // due soon
    .bind(3_i64) // checklist_done
    .bind(8_i64) // checklist_total
    .bind(1_i64) // comment_count
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 7: In progress — "Plan Lisbon trip", P2, travel+family labels, cover #d6e4e8,
    //         checklist 6/11, 8 comments, 4 attachments, me+alex+jamie
    // (design-ref c11: cover #d6e4e8, travel+family, P2, normal due, 6/11, 8 comments, 4 attachments, me+al+ja)
    let card7_id = Uuid::now_v7().to_string();
    let pos7 = next_card_position(&mut last_card_pos[2]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, cover, position, \
         priority, due_at, checklist_done, checklist_total, comment_count, attachment_count, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(&card7_id)
    .bind(&list_ids[2])
    .bind(&board_id)
    .bind(7_i64)
    .bind("Plan Lisbon trip")
    .bind("Summer vacation planning")
    .bind("#d6e4e8") // cover
    .bind(pos7.to_string())
    .bind("P2")
    .bind(now + 30 * 24 * 3600 * 1000_i64) // due in 30 days
    .bind(6_i64)  // checklist_done
    .bind(11_i64) // checklist_total
    .bind(8_i64)  // comment_count
    .bind(4_i64)  // attachment_count
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 8: Done — "Order birthday cake", done=1
    let card8_id = Uuid::now_v7().to_string();
    let pos8 = next_card_position(&mut last_card_pos[3]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, position, \
         priority, due_at, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 0, ?, ?)",
    )
    .bind(&card8_id)
    .bind(&list_ids[3])
    .bind(&board_id)
    .bind(8_i64)
    .bind("Order birthday cake")
    .bind(Option::<String>::None)
    .bind(pos8.to_string())
    .bind("P2")
    .bind(Option::<i64>::None)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 9: Done — "Pay electricity bill", done=1
    let card9_id = Uuid::now_v7().to_string();
    let pos9 = next_card_position(&mut last_card_pos[3]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, position, \
         priority, due_at, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 0, ?, ?)",
    )
    .bind(&card9_id)
    .bind(&list_ids[3])
    .bind(&board_id)
    .bind(9_i64)
    .bind("Pay electricity bill")
    .bind(Option::<String>::None)
    .bind(pos9.to_string())
    .bind(Option::<String>::None)
    .bind(Option::<i64>::None)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Update next_card_num to 10 (past all 9 seeded cards)
    sqlx::query("UPDATE boards SET next_card_num = ? WHERE id = ?")
        .bind(10_i64)
        .bind(&board_id)
        .execute(&mut *tx)
        .await?;

    // ------------------------------------------------------------------
    // card_labels links (D-09)
    // ------------------------------------------------------------------
    // Card 1 (Buy groceries) → home label
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card1_id)
        .bind(label_home_id)
        .execute(&mut *tx)
        .await?;

    // Card 2 (Call plumber) → urgent label
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card2_id)
        .bind(label_urgent_id)
        .execute(&mut *tx)
        .await?;

    // Card 4 (File expense reports) → finance label
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card4_id)
        .bind(label_finance_id)
        .execute(&mut *tx)
        .await?;

    // Card 5 (Fix garage door) → urgent + home labels
    // (design-ref c9: urgent, home)
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card5_id)
        .bind(label_urgent_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card5_id)
        .bind(label_home_id)
        .execute(&mut *tx)
        .await?;

    // Card 6 (Plan holiday menu) → home + errand labels
    // (design-ref c6: home, errand)
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card6_id)
        .bind(label_home_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card6_id)
        .bind(label_errand_id)
        .execute(&mut *tx)
        .await?;

    // Card 7 (Plan Lisbon trip) → travel + family labels
    // (design-ref c11: travel, family)
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card7_id)
        .bind(label_travel_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card7_id)
        .bind(label_family_id)
        .execute(&mut *tx)
        .await?;

    // ------------------------------------------------------------------
    // Checklist on card 6 "Plan holiday menu" (D-09)
    // ------------------------------------------------------------------
    let checklist_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO checklists (id, card_id, title, position) VALUES (?, ?, ?, ?)",
    )
    .bind(&checklist_id)
    .bind(&card6_id)
    .bind("Dishes")
    .bind(0_i64)
    .execute(&mut *tx)
    .await?;

    // checklist_item 1 — not done
    let ci1_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO checklist_items (id, checklist_id, text, done, position) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&ci1_id)
    .bind(&checklist_id)
    .bind("Turkey")
    .bind(0_i64)
    .bind(0_i64)
    .execute(&mut *tx)
    .await?;

    // checklist_item 2 — done
    let ci2_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO checklist_items (id, checklist_id, text, done, position) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&ci2_id)
    .bind(&checklist_id)
    .bind("Mashed potatoes")
    .bind(1_i64)
    .bind(1_i64)
    .execute(&mut *tx)
    .await?;

    // ------------------------------------------------------------------
    // Comments (D-09)
    // ------------------------------------------------------------------
    // Card 5 (Fix garage door) — 1 comment (design-ref c9)
    let comment1_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO comments (id, card_id, author_id, body, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&comment1_id)
    .bind(&card5_id)
    .bind(&user_id)
    .bind("Called a repair service, they come Thursday.")
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 6 (Plan holiday menu) — 1 comment (design-ref c6)
    let comment2_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO comments (id, card_id, author_id, body, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&comment2_id)
    .bind(&card6_id)
    .bind(&user_id)
    .bind("Found a great recipe online for the stuffing.")
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // ------------------------------------------------------------------
    // card_members (D-09)
    // Alex and Jamie are board members too (for card_members FK to users)
    // ------------------------------------------------------------------
    // Add Alex and Jamie as board members (so they can be card members)
    sqlx::query(
        "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, ?)",
    )
    .bind(&board_id)
    .bind(&alex_id)
    .bind("member")
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO board_members (board_id, user_id, role) VALUES (?, ?, ?)",
    )
    .bind(&board_id)
    .bind(&jamie_id)
    .bind("member")
    .execute(&mut *tx)
    .await?;

    // Card 5 (Fix garage door): me only (design-ref c9: me)
    sqlx::query("INSERT INTO card_members (card_id, user_id) VALUES (?, ?)")
        .bind(&card5_id)
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;

    // Card 6 (Plan holiday menu): me + alex (design-ref c6: me, al)
    sqlx::query("INSERT INTO card_members (card_id, user_id) VALUES (?, ?)")
        .bind(&card6_id)
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO card_members (card_id, user_id) VALUES (?, ?)")
        .bind(&card6_id)
        .bind(&alex_id)
        .execute(&mut *tx)
        .await?;

    // Card 7 (Plan Lisbon trip): me + alex + jamie (design-ref c11: me, al, ja)
    sqlx::query("INSERT INTO card_members (card_id, user_id) VALUES (?, ?)")
        .bind(&card7_id)
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO card_members (card_id, user_id) VALUES (?, ?)")
        .bind(&card7_id)
        .bind(&alex_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO card_members (card_id, user_id) VALUES (?, ?)")
        .bind(&card7_id)
        .bind(&jamie_id)
        .execute(&mut *tx)
        .await?;

    // ------------------------------------------------------------------
    // Commit the transaction
    // ------------------------------------------------------------------
    tx.commit().await?;

    Ok(())
}
