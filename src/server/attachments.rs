/// Axum multipart upload + download handlers for card attachments (DETAIL-08).
///
/// Upload route:  POST /api/attachments/:board_id/:card_id
/// Download route: GET /api/attachments/:board_id/:card_id/:key
///
/// Both routes are auth-gated (board membership required). Upload enforces a 10 MB size limit
/// via DefaultBodyLimit::max (T-05-19). Storage key is server-generated UUID — never derived
/// from the user-supplied filename (T-05-18). Download resolves the object key from the stored
/// attachments row, never from raw user input (T-05-21).

#[cfg(feature = "ssr")]
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
#[cfg(feature = "ssr")]
use crate::auth::helpers::AuthSession;
#[cfg(feature = "ssr")]
use crate::server::state::AppState;

// ---------------------------------------------------------------------------
// Upload handler
// ---------------------------------------------------------------------------

/// POST /api/attachments/:board_id/:card_id
///
/// Auth: bearer session required.
/// Board-membership: SELECT FROM board_members (require_board_member uses leptos_axum::extract,
///   which is unavailable in a plain Axum handler — replicate the SELECT directly, T-05-20).
/// Card scope: verify the card belongs to board_id (prevents cross-board IDOR).
/// Size: enforced by DefaultBodyLimit::max(10 MB) on the route layer; returns 413 from Axum.
/// Storage key: {card_id}/{uuid}.{ext} — UUIDs only (T-05-18, path-traversal safe).
/// Returns: JSON Attachment on success; JSON error on failure.
#[cfg(feature = "ssr")]
pub async fn upload_attachment_handler(
    State(state): State<AppState>,
    Path((board_id, card_id)): Path<(String, String)>,
    auth_session: AuthSession,
    mut multipart: axum::extract::Multipart,
) -> Response {
    use uuid::Uuid;
    use object_store::{ObjectStore, ObjectStoreExt, path::Path as StorePath};
    use crate::api::card_detail_api::record_attachment_inner;

    // 1. Require authenticated user (T-05-20)
    let user = match auth_session.user {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "authentication required"})),
            ).into_response();
        }
    };

    // 2. Verify board membership (T-05-20) — replicate require_board_member's SELECT
    let is_member: Option<String> = match sqlx::query_scalar(
        "SELECT role FROM board_members WHERE board_id = ? AND user_id = ?",
    )
    .bind(&board_id)
    .bind(&user.id)
    .fetch_optional(&state.read_pool.0)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("upload_attachment board_members query error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "temporarily unavailable"})),
            ).into_response();
        }
    };

    if is_member.is_none() {
        // D-12: generic error — does not reveal board existence to non-members
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "board not found"})),
        ).into_response();
    }

    // 3. Verify the card belongs to this board (cross-board IDOR gate, T-05-18)
    let card_on_board: Option<i64> = match sqlx::query_scalar(
        "SELECT 1 FROM cards WHERE id = ? AND board_id = ?",
    )
    .bind(&card_id)
    .bind(&board_id)
    .fetch_optional(&state.read_pool.0)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("upload_attachment card scope check error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "temporarily unavailable"})),
            ).into_response();
        }
    };

    if card_on_board.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "card not found"})),
        ).into_response();
    }

    // 4. Read the first multipart field (the file)
    let mut file_name_display = String::from("file");
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut total_size: usize = 0;

    while let Ok(Some(mut field)) = multipart.next_field().await {
        // Capture the display filename from the Content-Disposition header if present
        if let Some(name) = field.file_name() {
            // Store only the basename for display — no path component (T-05-18)
            let base = std::path::Path::new(name)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file");
            file_name_display = base.to_string();
        }

        let mut buf = Vec::new();
        while let Ok(Some(chunk)) = field.chunk().await {
            total_size += chunk.len();
            // Manual size check (belt-and-suspenders: DefaultBodyLimit is the primary gate)
            if total_size > 10 * 1024 * 1024 {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(serde_json::json!({"error": "Upload failed. Check file size and try again."})),
                ).into_response();
            }
            buf.extend_from_slice(&chunk);
        }
        file_bytes = Some(buf);
        break; // Only process the first file field
    }

    let bytes = match file_bytes {
        Some(b) => b,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "no file uploaded"})),
            ).into_response();
        }
    };

    // 5. Generate a UUID storage key — NEVER the user-supplied filename (T-05-18)
    let ext = std::path::Path::new(&file_name_display)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e))
        .unwrap_or_default();
    let uuid_key = Uuid::now_v7().to_string();
    let storage_key = format!("{}/{}{}", card_id, uuid_key, ext);
    let store_path = StorePath::from(storage_key.as_str());

    // 6. Persist bytes via object_store (T-05-18: path is server-controlled)
    let payload = object_store::PutPayload::from(bytes);
    if let Err(e) = state.storage.put(&store_path, payload).await {
        tracing::error!("upload_attachment object_store put error: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Upload failed. Check file size and try again."})),
        ).into_response();
    }

    // 7. Insert the attachments row + bump attachment_count in one transaction
    //    URL is the download path — constructed server-side from board_id/card_id/uuid_key
    let filename_with_ext = format!("{}{}", uuid_key, ext);
    let download_url = format!("/api/attachments/{}/{}/{}", board_id, card_id, filename_with_ext);

    match record_attachment_inner(
        &state.write_pool.0,
        &card_id,
        &user.id,
        &file_name_display,
        &download_url,
        total_size as i64,
    )
    .await
    {
        Ok(attachment) => (StatusCode::CREATED, Json(attachment)).into_response(),
        Err(e) => {
            tracing::error!("upload_attachment record_attachment_inner error: {e}");
            // Best-effort cleanup of the stored object (ignore errors — orphan is non-critical)
            let _ = state.storage.delete(&store_path).await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Upload failed. Check file size and try again."})),
            ).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Download handler
// ---------------------------------------------------------------------------

/// GET /api/attachments/:board_id/:card_id/:key
///
/// Auth: session required (T-05-20).
/// Board-membership: same SELECT-based check as upload (T-05-20).
/// Key resolution: look up the stored attachments row whose url matches
///   `/api/attachments/{board_id}/{card_id}/{key}` — only serves known objects (T-05-21).
/// Content-Disposition: attachment (T-05-22 — forces download, no inline execution).
#[cfg(feature = "ssr")]
pub async fn download_attachment_handler(
    State(state): State<AppState>,
    Path((board_id, card_id, key)): Path<(String, String, String)>,
    auth_session: AuthSession,
) -> Response {
    use object_store::{ObjectStore, ObjectStoreExt, path::Path as StorePath};

    // 1. Require authenticated user
    let user = match auth_session.user {
        Some(u) => u,
        None => {
            return (StatusCode::UNAUTHORIZED, "authentication required").into_response();
        }
    };

    // 2. Verify board membership
    let is_member: Option<String> = match sqlx::query_scalar(
        "SELECT role FROM board_members WHERE board_id = ? AND user_id = ?",
    )
    .bind(&board_id)
    .bind(&user.id)
    .fetch_optional(&state.read_pool.0)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("download_attachment board_members query error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "temporarily unavailable").into_response();
        }
    };

    if is_member.is_none() {
        return (StatusCode::NOT_FOUND, "board not found").into_response();
    }

    // 3. Resolve the attachment row by the expected download URL (T-05-21).
    //    Do NOT serve arbitrary keys — only keys referenced by a stored attachments row.
    let expected_url = format!("/api/attachments/{}/{}/{}", board_id, card_id, key);

    let row: Option<(String, String, i64)> = match sqlx::query_as(
        "SELECT id, filename, size_bytes FROM attachments WHERE card_id = ? AND url = ?",
    )
    .bind(&card_id)
    .bind(&expected_url)
    .fetch_optional(&state.read_pool.0)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("download_attachment row lookup error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "temporarily unavailable").into_response();
        }
    };

    let (_attachment_id, display_filename, _size) = match row {
        Some(r) => r,
        None => {
            return (StatusCode::NOT_FOUND, "attachment not found").into_response();
        }
    };

    // 4. Retrieve bytes from object_store using the card_id/key path
    let storage_key = format!("{}/{}", card_id, key);
    let store_path = StorePath::from(storage_key.as_str());

    let get_result = match state.storage.get(&store_path).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("download_attachment object_store get error: {e}");
            return (StatusCode::NOT_FOUND, "attachment not found").into_response();
        }
    };

    let bytes = match get_result.bytes().await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("download_attachment bytes read error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "read error").into_response();
        }
    };

    // 5. Serve with Content-Disposition: attachment (T-05-22 — no inline execution)
    //    Sanitize display_filename for Content-Disposition header (strip control chars + quotes)
    let safe_filename: String = display_filename
        .chars()
        .filter(|c| *c != '"' && *c != '\\' && !c.is_control())
        .collect();
    let content_disposition = format!("attachment; filename=\"{}\"", safe_filename);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_DISPOSITION, content_disposition)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, bytes.len().to_string())
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
