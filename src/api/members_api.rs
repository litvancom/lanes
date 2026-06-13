use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MemberRow {
    pub user_id: String,
    pub display_name: String,
    pub email: String,
    pub role: String,
}

/// List a board's members (owner-only). Owner row first.
#[server]
pub async fn list_board_members(board_id: String) -> Result<Vec<MemberRow>, ServerFnError> {
    use crate::auth::helpers::require_board_owner;
    use crate::server::state::AppState;
    let state = expect_context::<AppState>();
    require_board_owner(&board_id, &state.read_pool.0).await?;
    let rows = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT u.id, u.display_name, u.email, bm.role \
         FROM board_members bm JOIN users u ON u.id = bm.user_id \
         WHERE bm.board_id = ? ORDER BY (bm.role = 'owner') DESC, u.display_name COLLATE NOCASE",
    )
    .bind(&board_id)
    .fetch_all(&state.read_pool.0)
    .await
    .map_err(|e| {
        tracing::error!("list_board_members error: {e}");
        ServerFnError::new("Failed to load members")
    })?;
    Ok(rows
        .into_iter()
        .map(|(user_id, display_name, email, role)| MemberRow {
            user_id,
            display_name,
            email,
            role,
        })
        .collect())
}

/// Change a member's access level (owner-only). Cannot target the owner; cannot set 'owner'.
#[server]
pub async fn set_member_role(
    board_id: String,
    user_id: String,
    role: String,
) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_owner;
    use crate::auth::role::Role;
    use crate::server::state::AppState;
    let state = expect_context::<AppState>();
    require_board_owner(&board_id, &state.read_pool.0).await?;
    let role = match Role::parse(&role) {
        Some(r) if r.is_invitable() => r.as_str().to_string(),
        _ => return Err(ServerFnError::new("Invalid access level")),
    };
    let res = sqlx::query(
        "UPDATE board_members SET role = ? WHERE board_id = ? AND user_id = ? AND role != 'owner'",
    )
    .bind(&role)
    .bind(&board_id)
    .bind(&user_id)
    .execute(&state.write_pool.0)
    .await
    .map_err(|e| {
        tracing::error!("set_member_role error: {e}");
        ServerFnError::new("Failed to update access")
    })?;
    if res.rows_affected() == 0 {
        return Err(ServerFnError::new("Cannot change this member"));
    }
    Ok(())
}

/// Remove a member from the board (owner-only). Cannot remove the owner.
#[server]
pub async fn remove_member(board_id: String, user_id: String) -> Result<(), ServerFnError> {
    use crate::auth::helpers::require_board_owner;
    use crate::server::state::AppState;
    let state = expect_context::<AppState>();
    require_board_owner(&board_id, &state.read_pool.0).await?;
    let res = sqlx::query(
        "DELETE FROM board_members WHERE board_id = ? AND user_id = ? AND role != 'owner'",
    )
    .bind(&board_id)
    .bind(&user_id)
    .execute(&state.write_pool.0)
    .await
    .map_err(|e| {
        tracing::error!("remove_member error: {e}");
        ServerFnError::new("Failed to remove member")
    })?;
    if res.rows_affected() == 0 {
        return Err(ServerFnError::new("Cannot remove this member"));
    }
    Ok(())
}
