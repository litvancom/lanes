//! REST API handler for `/api/v1/workspaces`.
//!
//! Lanes v1 has a single-workspace model: each user has exactly one implicit workspace
//! that contains all the boards they are a member of.  `GET /api/v1/workspaces` returns
//! that workspace descriptor.

#![cfg(feature = "ssr")]

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::{
    models::rest_dto::WorkspaceDto,
    server::{
        rest_api::auth::ApiUser,
        state::AppState,
    },
};

// ---------------------------------------------------------------------------
// GET /api/v1/workspaces
// ---------------------------------------------------------------------------

/// Return the caller's workspace descriptor.
///
/// In Lanes v1 each user has exactly one implicit workspace — there is no `workspaces`
/// table.  The response contains the authenticated user's display name and a count of
/// the boards they are a member of.
#[utoipa::path(
    get,
    path = "/api/v1/workspaces",
    responses(
        (status = 200, description = "Workspace descriptor", body = WorkspaceDto),
        (status = 401, description = "Unauthorized"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn get_workspace(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
) -> Response {
    // Count the boards the user is a member of (non-archived only for the public count)
    let board_count: Result<i64, sqlx::Error> = sqlx::query_scalar(
        r#"SELECT COUNT(*)
           FROM boards b
           JOIN board_members m ON m.board_id = b.id
           WHERE m.user_id = ? AND b.archived = 0"#,
    )
    .bind(&user.id)
    .fetch_one(&state.read_pool.0)
    .await;

    match board_count {
        Ok(count) => {
            let dto = WorkspaceDto {
                id: user.id.clone(),
                display_name: user.display_name.clone(),
                board_count: count,
            };
            (StatusCode::OK, Json(dto)).into_response()
        }
        Err(e) => {
            tracing::error!("get_workspace REST error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
        }
    }
}
