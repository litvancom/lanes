//! Axum WebSocket upgrade handler for per-board realtime sync (RT-01).
//!
//! Route: `GET /ws/board/:id`
//!
//! Auth: session cookie required (T-6-01: 401 if absent).
//! Membership: board_members SELECT (T-6-01: 403 if non-member).
//! Per-connection `client_id` UUID assigned at upgrade time (D-05/Flag 2).
//!
//! A single `tokio::select!` loop races four futures:
//!   1. socket.recv()     — client→server (heartbeat, typing, presence)
//!   2. board_rx.recv()   — board mutation events → WsEnvelope::Board
//!   3. user_rx.recv()    — per-user notifications → WsEnvelope::User
//!   4. pres_rx.recv()    — presence events → WsEnvelope::Presence
//!
//! On loop exit (any arm errors or close): explicit Pitfall-1 cleanup.
//! RecvError::Lagged on board_rx → send Refresh + resubscribe (Pitfall 2).

#[cfg(feature = "ssr")]
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
};
#[cfg(feature = "ssr")]
use axum::extract::ws::{Message, WebSocket};
#[cfg(feature = "ssr")]
use crate::auth::helpers::AuthSession;
#[cfg(feature = "ssr")]
use crate::auth::models::AuthUser;
#[cfg(feature = "ssr")]
use crate::server::state::AppState;
#[cfg(feature = "ssr")]
use crate::models::events::{BoardEvent, WsEnvelope};

/// GET /ws/board/:id
///
/// Upgrades to a WebSocket after verifying authentication and board membership.
/// Returns 401 if the user is not authenticated (T-6-01).
/// Returns 403 if the user is not a member of the board (T-6-01).
#[cfg(feature = "ssr")]
pub async fn ws_board_handler(
    ws: WebSocketUpgrade,
    Path(board_id): Path<String>,
    State(state): State<AppState>,
    auth_session: AuthSession,
) -> impl IntoResponse {
    // 1. Require authentication (T-6-01)
    let user = match auth_session.user {
        Some(u) => u,
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    // 2. Replicate board membership check (T-6-01).
    //    leptos_axum::extract is unavailable in plain Axum handlers — same pattern as
    //    upload_attachment_handler (attachments.rs). SELECT directly from read_pool.
    let role: Option<String> = match sqlx::query_scalar(
        "SELECT role FROM board_members WHERE board_id = ? AND user_id = ?",
    )
    .bind(&board_id)
    .bind(&user.id)
    .fetch_optional(&state.read_pool.0)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("ws_board_handler board_members query error: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if role.is_none() {
        // T-6-01: non-member (or board does not exist) → 403 before WS upgrade
        return StatusCode::FORBIDDEN.into_response();
    }

    // 3. Upgrade to WebSocket
    ws.on_upgrade(move |socket| handle_ws(socket, board_id, user, state))
}

/// WebSocket session handler.
///
/// Owns all three channel Receivers for the lifetime of the connection.
/// The `tokio::select!` loop exits on any socket error, close frame, or channel shutdown.
/// Explicit cleanup (Pitfall 1) on exit.
#[cfg(feature = "ssr")]
async fn handle_ws(mut socket: WebSocket, board_id: String, user: AuthUser, state: AppState) {
    use tokio::sync::broadcast::error::RecvError;

    // Assign a per-connection UUID. This is server-generated — the client cannot
    // influence it. It is returned in the Connected handshake and the client stores
    // it as own_client_id to suppress highlight for its own events (D-05/Flag 2).
    let client_id = uuid::Uuid::new_v4().to_string();

    // Subscribe to all three channels BEFORE sending the snapshot (Pitfall 3):
    // subscribing first ensures we don't miss any events emitted between
    // snapshot build and the subscribe call.
    let mut board_rx = state.board_rooms.subscribe(&board_id);
    let mut user_rx = state.user_notifs.subscribe(&user.id);
    let mut pres_rx = state.presence.subscribe(&board_id);

    // --- Initial Connected handshake ---
    let current_seq = state.board_rooms.current_seq(&board_id);
    let handshake = WsEnvelope::Board {
        payload: BoardEvent::Connected {
            client_id: client_id.clone(),
            board_seq: current_seq,
        },
    };
    let handshake_json = match serde_json::to_string(&handshake) {
        Ok(j) => j,
        Err(e) => {
            tracing::error!("ws handshake serialize error: {e}");
            state.user_notifs.remove(&user.id);
            return;
        }
    };
    if socket
        .send(Message::Text(handshake_json.into()))
        .await
        .is_err()
    {
        // Client disconnected immediately after upgrade (rare but valid)
        state.user_notifs.remove(&user.id);
        state.presence.leave(&board_id, &user.id);
        return;
    }

    // --- Presence snapshot (Pitfall 3: already subscribed above) ---
    let snapshot = state.presence.snapshot(&board_id);
    if let Ok(snap_json) = serde_json::to_string(&WsEnvelope::Presence { payload: snapshot }) {
        let _ = socket.send(Message::Text(snap_json.into())).await;
    }

    // --- Mark viewer as present (broadcasts ViewerJoined to other viewers) ---
    state.presence.join(&board_id, &user, &client_id);

    // --- Main relay loop ---
    loop {
        tokio::select! {
            // Client → Server messages
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_client_message(&text, &board_id, &user, &state).await;
                    }
                    // Ping/Pong — axum handles pong automatically; no action needed
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                    // Close frame or socket error — exit loop
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    // Binary frames: ignore (protocol only uses text JSON)
                    Some(Ok(Message::Binary(_))) => {}
                }
            }

            // Board mutation events → relay as WsEnvelope::Board
            event = board_rx.recv() => {
                match event {
                    Ok(ev) => {
                        if let Ok(json) = serde_json::to_string(&WsEnvelope::Board { payload: ev }) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(RecvError::Lagged(_)) => {
                        // Client fell behind the ring buffer (T-6-05 mitigation):
                        // send Refresh so the client fetches full board state,
                        // then resubscribe to the current tail (Pitfall 2).
                        let refresh = WsEnvelope::Board { payload: BoardEvent::Refresh };
                        if let Ok(json) = serde_json::to_string(&refresh) {
                            let _ = socket.send(Message::Text(json.into())).await;
                        }
                        board_rx = state.board_rooms.subscribe(&board_id);
                    }
                    Err(RecvError::Closed) => break,
                }
            }

            // Per-user notification events → relay as WsEnvelope::User
            notif = user_rx.recv() => {
                match notif {
                    Some(ev) => {
                        if let Ok(json) = serde_json::to_string(&WsEnvelope::User { payload: ev }) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    // Sender dropped (server shutting down) — clean exit
                    None => break,
                }
            }

            // Presence events for this board → relay as WsEnvelope::Presence
            pres = pres_rx.recv() => {
                match pres {
                    Ok(ev) => {
                        if let Ok(json) = serde_json::to_string(&WsEnvelope::Presence { payload: ev }) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(RecvError::Lagged(_)) => {
                        // Presence lag is low-stakes: send a fresh snapshot instead of Refresh
                        let snapshot = state.presence.snapshot(&board_id);
                        if let Ok(json) = serde_json::to_string(&WsEnvelope::Presence { payload: snapshot }) {
                            let _ = socket.send(Message::Text(json.into())).await;
                        }
                        pres_rx = state.presence.subscribe(&board_id);
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }

    // --- Explicit cleanup (Pitfall 1: prevent Sender leak in registries) ---
    // board_rx and pres_rx are dropped automatically by going out of scope.
    // user_rx is also dropped, but we must remove the dead Sender from the registry.
    state.user_notifs.remove(&user.id);
    // presence.leave broadcasts ViewerLeft to remaining viewers and removes from viewers map.
    state.presence.leave(&board_id, &user.id);
}

/// Process a client→server message from inside the relay loop.
///
/// Client messages are small JSON objects with a `"type"` field:
///   - `{"type":"heartbeat"}` — presence keepalive (D-13)
///   - `{"type":"presence_join"}` — tab became visible (D-12)
///   - `{"type":"presence_leave"}` — tab became hidden (D-12)
///   - `{"type":"typing", "card_id":"...", "is_typing":true}` — D-10
///
/// All branches are stubbed in 06-01. Plan 06-04 implements the full logic.
#[cfg(feature = "ssr")]
async fn handle_client_message(
    text: &str,
    board_id: &str,
    user: &AuthUser,
    state: &AppState,
) {
    // Parse just the "type" field to route the message
    let msg_type: Option<String> = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string));

    match msg_type.as_deref() {
        Some("heartbeat") => {
            // Update viewer's last_heartbeat (stub: 06-04 wires full logic)
            state.presence.heartbeat(board_id, &user.id);
        }
        Some("presence_join") => {
            // Tab became visible again (stub: 06-04)
            tracing::debug!(user_id = %user.id, board_id, "presence_join received");
        }
        Some("presence_leave") => {
            // Tab hidden — best-effort early leave (stub: 06-04)
            tracing::debug!(user_id = %user.id, board_id, "presence_leave received");
        }
        Some("typing") => {
            // Typing indicator (stub: 06-04)
            tracing::debug!(user_id = %user.id, board_id, "typing received");
        }
        Some(other) => {
            tracing::debug!(user_id = %user.id, board_id, msg_type = other, "unknown client message type");
        }
        None => {
            tracing::warn!(user_id = %user.id, board_id, "malformed client message (no type): {text}");
        }
    }
}
