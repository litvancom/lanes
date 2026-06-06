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

    // All fixture inserts are inside a single transaction (T-02-02).
    let mut tx = write_pool.begin().await?;

    // ------------------------------------------------------------------
    // Timestamps — epoch millis UTC (D-03)
    // ------------------------------------------------------------------
    let now: i64 = crate::server::now_millis().expect("time went backwards");

    // ------------------------------------------------------------------
    // User: Mira (D-09)
    // ------------------------------------------------------------------
    let user_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, avatar_color, created_at) \
         VALUES (?, ?, NULL, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind("mira@example.com")
    .bind("Mira")
    .bind("#7c5cff")
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
    // ------------------------------------------------------------------
    let list_names = ["Inbox", "This week", "In progress", "Done"];
    let mut list_ids: Vec<String> = Vec::with_capacity(4);
    let mut prev_pos = FractionalIndex::default();

    for (i, name) in list_names.iter().enumerate() {
        let list_id = Uuid::now_v7().to_string();
        let pos = if i == 0 {
            FractionalIndex::default()
        } else {
            FractionalIndex::new_after(&prev_pos)
        };
        let pos_str = pos.to_string();

        sqlx::query(
            "INSERT INTO lists (id, board_id, name, position, archived) VALUES (?, ?, ?, ?, 0)",
        )
        .bind(&list_id)
        .bind(&board_id)
        .bind(name)
        .bind(&pos_str)
        .execute(&mut *tx)
        .await?;

        prev_pos = pos;
        list_ids.push(list_id);
    }

    // list_ids[0] = Inbox, [1] = This week, [2] = In progress, [3] = Done

    // ------------------------------------------------------------------
    // Labels (2) — board-scoped (D-09)
    // ------------------------------------------------------------------
    let label_urgent_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO labels (id, board_id, name, color) VALUES (?, ?, ?, ?)",
    )
    .bind(&label_urgent_id)
    .bind(&board_id)
    .bind("Urgent")
    .bind("oklch(0.65 0.25 25)")
    .execute(&mut *tx)
    .await?;

    let label_home_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO labels (id, board_id, name, color) VALUES (?, ?, ?, ?)",
    )
    .bind(&label_home_id)
    .bind(&board_id)
    .bind("Home")
    .bind("oklch(0.65 0.2 145)")
    .execute(&mut *tx)
    .await?;

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

    // Card 1: Inbox — "Buy groceries", P3, Home label
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

    // Card 2: Inbox — "Call plumber", P1, Urgent label
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

    // Card 4: This week — "Research new laptop", P3, Home label, due
    let card4_id = Uuid::now_v7().to_string();
    let pos4 = next_card_position(&mut last_card_pos[1]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, position, \
         priority, due_at, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(&card4_id)
    .bind(&list_ids[1])
    .bind(&board_id)
    .bind(4_i64)
    .bind("Research new laptop")
    .bind("Check specs and prices")
    .bind(pos4.to_string())
    .bind("P3")
    .bind(now + 5 * 24 * 3600 * 1000_i64) // due in 5 days
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 5: In progress — "Fix garage door", P1, Urgent label, member
    let card5_id = Uuid::now_v7().to_string();
    let pos5 = next_card_position(&mut last_card_pos[2]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, position, \
         priority, due_at, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(&card5_id)
    .bind(&list_ids[2])
    .bind(&board_id)
    .bind(5_i64)
    .bind("Fix garage door")
    .bind("Spring is broken")
    .bind(pos5.to_string())
    .bind("P1")
    .bind(now + 3 * 24 * 3600 * 1000_i64)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 6: In progress — "Plan holiday menu", P2, checklist + comment
    let card6_id = Uuid::now_v7().to_string();
    let pos6 = next_card_position(&mut last_card_pos[2]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, position, \
         priority, due_at, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(&card6_id)
    .bind(&list_ids[2])
    .bind(&board_id)
    .bind(6_i64)
    .bind("Plan holiday menu")
    .bind("Thanksgiving menu planning")
    .bind(pos6.to_string())
    .bind("P2")
    .bind(now + 7 * 24 * 3600 * 1000_i64)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 7: Done — "Order birthday cake", done=1
    let card7_id = Uuid::now_v7().to_string();
    let pos7 = next_card_position(&mut last_card_pos[3]);
    sqlx::query(
        "INSERT INTO cards (id, list_id, board_id, card_num, title, description, position, \
         priority, due_at, done, archived, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 0, ?, ?)",
    )
    .bind(&card7_id)
    .bind(&list_ids[3])
    .bind(&board_id)
    .bind(7_i64)
    .bind("Order birthday cake")
    .bind(Option::<String>::None)
    .bind(pos7.to_string())
    .bind("P2")
    .bind(Option::<i64>::None)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Card 8: Done — "Pay electricity bill", done=1
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
    .bind("Pay electricity bill")
    .bind(Option::<String>::None)
    .bind(pos8.to_string())
    .bind(Option::<String>::None)
    .bind(Option::<i64>::None)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Update next_card_num to 9 (past all seeded cards)
    sqlx::query("UPDATE boards SET next_card_num = ? WHERE id = ?")
        .bind(9_i64)
        .bind(&board_id)
        .execute(&mut *tx)
        .await?;

    // ------------------------------------------------------------------
    // card_labels links (D-09)
    // ------------------------------------------------------------------
    // Card 1 (Buy groceries) → Home label
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card1_id)
        .bind(&label_home_id)
        .execute(&mut *tx)
        .await?;

    // Card 2 (Call plumber) → Urgent label
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card2_id)
        .bind(&label_urgent_id)
        .execute(&mut *tx)
        .await?;

    // Card 5 (Fix garage door) → Urgent label
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card5_id)
        .bind(&label_urgent_id)
        .execute(&mut *tx)
        .await?;

    // Card 4 (Research new laptop) → Home label
    sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES (?, ?)")
        .bind(&card4_id)
        .bind(&label_home_id)
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
    // ------------------------------------------------------------------
    sqlx::query("INSERT INTO card_members (card_id, user_id) VALUES (?, ?)")
        .bind(&card5_id)
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;

    sqlx::query("INSERT INTO card_members (card_id, user_id) VALUES (?, ?)")
        .bind(&card6_id)
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;

    // ------------------------------------------------------------------
    // Commit the transaction
    // ------------------------------------------------------------------
    tx.commit().await?;

    Ok(())
}
