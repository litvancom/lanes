//! REST API handlers for `/api/v1/boards/{board_id}/cards`.
//!
//! Board-membership enforced by replicating the board_members SELECT (D-16 / no IDOR).
//! Mutations publish BoardEvents so web clients stay live (D-20).

#![cfg(feature = "ssr")]

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::{
    models::{
        events::{BoardEvent, CardPatch, CardSummary},
        rest_dto::{CardDto, CreateCardReq, MoveCardReq, PaginationParams, UpdateCardReq},
    },
    server::{rest_api::{auth::ApiUser, boards::require_member}, state::AppState},
};

// ---------------------------------------------------------------------------
// GET /api/v1/boards/{board_id}/cards
// ---------------------------------------------------------------------------

/// List all non-archived cards in a board.
#[utoipa::path(
    get,
    path = "/api/v1/boards/{board_id}/cards",
    params(
        ("board_id" = String, Path, description = "Board ID"),
        PaginationParams,
    ),
    responses(
        (status = 200, description = "Card list", body = Vec<CardDto>),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Board not found or not a member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn list_cards(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path(board_id): Path<String>,
    Query(pagination): Query<PaginationParams>,
) -> Response {
    if let Err(resp) = require_member(&state.read_pool.0, &board_id, &user.id).await {
        return resp;
    }

    let limit = pagination.limit.unwrap_or(50).min(200);

    type CardRow = (String, String, String, i64, String, String, Option<String>, Option<i64>, bool, bool, i64, i64);
    let rows: Result<Vec<CardRow>, sqlx::Error> = sqlx::query_as(
        r#"SELECT id, board_id, list_id, card_num, title, position,
                  priority, due_at,
                  CAST(done AS BOOLEAN),
                  CAST(archived AS BOOLEAN),
                  created_at, updated_at
           FROM cards
           WHERE board_id = ? AND archived = 0
           ORDER BY list_id, position ASC
           LIMIT ?"#,
    )
    .bind(&board_id)
    .bind(limit)
    .fetch_all(&state.read_pool.0)
    .await;

    match rows {
        Ok(rows) => {
            let dtos: Vec<CardDto> = rows
                .into_iter()
                .map(|(id, board_id, list_id, card_num, title, position, priority, due_at, done, archived, created_at, updated_at)| {
                    CardDto { id, board_id, list_id, card_num, title, position, priority, due_at, done, archived, created_at, updated_at }
                })
                .collect();
            (StatusCode::OK, Json(dtos)).into_response()
        }
        Err(e) => {
            tracing::error!("list_cards REST error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/v1/boards/{board_id}/cards
// ---------------------------------------------------------------------------

/// Create a new card at the end of a list.
#[utoipa::path(
    post,
    path = "/api/v1/boards/{board_id}/cards",
    params(("board_id" = String, Path, description = "Board ID")),
    request_body = CreateCardReq,
    responses(
        (status = 201, description = "Card created", body = CardDto),
        (status = 400, description = "Validation error"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Board not found or not a member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn create_card(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path(board_id): Path<String>,
    Json(body): Json<CreateCardReq>,
) -> Response {
    use crate::api::card_api::{create_card_inner, next_card_position};
    use crate::api::list_api::board_id_for_list;

    if let Err(resp) = require_member(&state.read_pool.0, &board_id, &user.id).await {
        return resp;
    }

    // Verify the target list belongs to this board (CR-02 — no cross-board card insertion)
    let owning_board = board_id_for_list(&state.read_pool.0, &body.list_id).await;
    match owning_board {
        Ok(Some(ref b)) if *b == board_id => {}
        Ok(_) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "list not found"}))).into_response(),
        Err(e) => {
            tracing::error!("board_id_for_list REST error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response();
        }
    }

    let position = match next_card_position(&state.read_pool.0, &body.list_id).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("next_card_position REST error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response();
        }
    };

    match create_card_inner(&state.write_pool.0, &board_id, &body.list_id, body.title, &position).await {
        Ok(card) => {
            // D-20: publish CardAdded
            state.board_rooms.publish_seq(&board_id, |seq| BoardEvent::CardAdded {
                board_seq: seq,
                client_id: "api".to_string(),
                card: CardSummary {
                    id: card.id.clone(),
                    list_id: card.list_id.clone(),
                    board_id: card.board_id.clone(),
                    card_num: card.card_num,
                    title: card.title.clone(),
                    position: card.position.clone(),
                    priority: card.priority.clone(),
                    due_at: card.due_at,
                    done: card.done,
                    cover: card.cover.clone(),
                    labels: card.labels.clone(),
                    member_ids: card.member_ids.clone(),
                },
            });

            // Fetch created_at / updated_at (create_card_inner doesn't return them).
            // Never fabricate epoch-0 on error: the row exists with correct values, so a
            // failed/raced follow-up read is a server fault, not a 1970 timestamp.
            let timestamps: Option<(i64, i64)> = match sqlx::query_as(
                "SELECT created_at, updated_at FROM cards WHERE id = ?",
            )
            .bind(&card.id)
            .fetch_optional(&state.read_pool.0)
            .await {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!("create_card timestamp re-fetch error: {e}");
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response();
                }
            };
            let (created_at, updated_at) = match timestamps {
                Some(ts) => ts,
                None => {
                    tracing::error!("create_card timestamp re-fetch returned no row for card {}", card.id);
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response();
                }
            };

            let dto = CardDto {
                id: card.id,
                board_id: card.board_id,
                list_id: card.list_id,
                card_num: card.card_num,
                title: card.title,
                position: card.position,
                priority: card.priority,
                due_at: card.due_at,
                done: card.done,
                archived: card.archived,
                created_at,
                updated_at,
            };
            (StatusCode::CREATED, Json(dto)).into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("cannot be empty") || msg.contains("or fewer") {
                (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg}))).into_response()
            } else {
                tracing::error!("create_card REST error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PATCH /api/v1/boards/{board_id}/cards/{card_id}
// ---------------------------------------------------------------------------

/// Update a card's title.
#[utoipa::path(
    patch,
    path = "/api/v1/boards/{board_id}/cards/{card_id}",
    params(
        ("board_id" = String, Path, description = "Board ID"),
        ("card_id" = String, Path, description = "Card ID"),
    ),
    request_body = UpdateCardReq,
    responses(
        (status = 200, description = "Updated card", body = CardDto),
        (status = 400, description = "Validation error"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Board not found or not a member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn update_card(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path((board_id, card_id)): Path<(String, String)>,
    Json(body): Json<UpdateCardReq>,
) -> Response {
    use crate::api::card_detail_api::update_card_title_inner;

    if let Err(resp) = require_member(&state.read_pool.0, &board_id, &user.id).await {
        return resp;
    }

    if let Some(title) = body.title {
        match update_card_title_inner(&state.write_pool.0, &board_id, &card_id, title.clone()).await {
            Ok(saved_title) => {
                // D-20: publish CardUpdated with title patch
                state.board_rooms.publish_seq(&board_id, |seq| BoardEvent::CardUpdated {
                    board_seq: seq,
                    client_id: "api".to_string(),
                    card_id: card_id.clone(),
                    patch: CardPatch {
                        title: Some(saved_title.clone()),
                        description: None,
                        cover: None,
                        done: None,
                        card_num: None,
                    },
                });
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("cannot be empty") || msg.contains("or fewer") {
                    return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg}))).into_response();
                }
                tracing::error!("update_card REST error: {e}");
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response();
            }
        }
    }

    // Fetch and return the updated card
    type CardRow = (String, String, String, i64, String, String, Option<String>, Option<i64>, bool, bool, i64, i64);
    let row: Result<Option<CardRow>, sqlx::Error> = sqlx::query_as(
        r#"SELECT id, board_id, list_id, card_num, title, position,
                  priority, due_at,
                  CAST(done AS BOOLEAN),
                  CAST(archived AS BOOLEAN),
                  created_at, updated_at
           FROM cards WHERE id = ? AND board_id = ?"#,
    )
    .bind(&card_id)
    .bind(&board_id)
    .fetch_optional(&state.read_pool.0)
    .await;

    match row {
        Ok(Some((id, board_id, list_id, card_num, title, position, priority, due_at, done, archived, created_at, updated_at))) => {
            let dto = CardDto { id, board_id, list_id, card_num, title, position, priority, due_at, done, archived, created_at, updated_at };
            (StatusCode::OK, Json(dto)).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "card not found"}))).into_response(),
        Err(e) => {
            tracing::error!("update_card fetch REST error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/v1/boards/{board_id}/cards/{card_id}/move
// ---------------------------------------------------------------------------

/// Move a card to a different list and/or position.
#[utoipa::path(
    post,
    path = "/api/v1/boards/{board_id}/cards/{card_id}/move",
    params(
        ("board_id" = String, Path, description = "Board ID"),
        ("card_id" = String, Path, description = "Card ID"),
    ),
    request_body = MoveCardReq,
    responses(
        (status = 204, description = "Card moved"),
        (status = 400, description = "Validation error"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Board not found or not a member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn move_card(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path((board_id, card_id)): Path<(String, String)>,
    Json(body): Json<MoveCardReq>,
) -> Response {
    use crate::api::card_api::move_card_inner;

    if let Err(resp) = require_member(&state.read_pool.0, &board_id, &user.id).await {
        return resp;
    }

    match move_card_inner(&state.write_pool.0, &board_id, &card_id, &body.to_list_id, &body.position).await {
        Ok(()) => {
            // D-20: publish CardMoved
            state.board_rooms.publish_seq(&board_id, |seq| BoardEvent::CardMoved {
                board_seq: seq,
                client_id: "api".to_string(),
                card_id: card_id.clone(),
                to_list_id: body.to_list_id.clone(),
                position: body.position.clone(),
            });
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("invalid position") || msg.contains("not on this board") || msg.contains("not found") {
                (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg}))).into_response()
            } else {
                tracing::error!("move_card REST error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/boards/{board_id}/cards/{card_id}
// ---------------------------------------------------------------------------

/// Archive a card (soft delete).
#[utoipa::path(
    delete,
    path = "/api/v1/boards/{board_id}/cards/{card_id}",
    params(
        ("board_id" = String, Path, description = "Board ID"),
        ("card_id" = String, Path, description = "Card ID"),
    ),
    responses(
        (status = 204, description = "Card archived"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Board not found or not a member"),
    ),
    security(("bearer_token" = [])),
)]
pub async fn delete_card(
    State(state): State<AppState>,
    ApiUser(user): ApiUser,
    Path((board_id, card_id)): Path<(String, String)>,
) -> Response {
    use crate::api::card_detail_api::archive_card_inner;

    if let Err(resp) = require_member(&state.read_pool.0, &board_id, &user.id).await {
        return resp;
    }

    match archive_card_inner(&state.write_pool.0, &board_id, &card_id).await {
        Ok(()) => {
            // D-20: publish CardArchived
            state.board_rooms.publish_seq(&board_id, |seq| BoardEvent::CardArchived {
                board_seq: seq,
                client_id: "api".to_string(),
                card_id: card_id.clone(),
            });
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            tracing::error!("delete_card REST error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "internal error"}))).into_response()
        }
    }
}
