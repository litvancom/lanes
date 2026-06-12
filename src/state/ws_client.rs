//! WASM WebSocket client task for per-board realtime sync (RT-01/RT-02).
//!
//! `spawn_ws_task` opens a WebSocket connection to `/ws/board/:id`, receives
//! `WsEnvelope` JSON messages, and patches the board's reactive signals in place.
//!
//! RT-02: The task wraps the connect in an exponential-backoff reconnect loop.
//! On a sequence-number gap, it calls `refresh_board` to atomically swap in
//! fresh board state without showing a spinner (stale-then-swap, D-03).
//!
//! The task is launched by `BoardPage` on mount and torn down when the component
//! unmounts via `on_cleanup` dropping the `WsHandle` (D-06 teardown).
//!
//! D-05 (self-echo suppression): `apply_board_event` compares the event's
//! `client_id` against `signals.own_client_id` to skip the highlight flash
//! for the originator's own moves.
//!
//! This module compiles under:
//!   - `#[cfg(target_arch = "wasm32")]` — the real implementation using web_sys
//!   - non-wasm (SSR, tests) — stub that returns a no-op WsHandle so lib compiles

use leptos::prelude::*;
use crate::routes::board::{BoardSignals, PresenceViewer, WsSendFn};
use crate::models::events::{BoardEvent, NotifEvent, PresenceEvent, WsEnvelope};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

// ---------------------------------------------------------------------------
// WsHandle — teardown token
// ---------------------------------------------------------------------------

/// A handle returned by `spawn_ws_task`. Dropping it stops the WS task.
///
/// The abort flag is checked in the message/close closures. On Drop it is set to true,
/// which causes the task to exit gracefully on the next event or timeout.
pub struct WsHandle {
    abort: Arc<AtomicBool>,
}

impl WsHandle {
    fn new(abort: Arc<AtomicBool>) -> Self {
        Self { abort }
    }

    /// Signal the task to abort.
    fn signal_abort(&self) {
        self.abort.store(true, Ordering::Relaxed);
    }
}

impl Drop for WsHandle {
    fn drop(&mut self) {
        self.signal_abort();
    }
}

// ---------------------------------------------------------------------------
// WASM implementation
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
pub fn spawn_ws_task(board_id: String, signals: BoardSignals) -> WsHandle {
    let abort = Arc::new(AtomicBool::new(false));
    let handle = WsHandle::new(Arc::clone(&abort));

    let abort_task = Arc::clone(&abort);
    wasm_bindgen_futures::spawn_local(async move {
        reconnect_loop(board_id, signals, abort_task).await;
    });

    handle
}

/// Exponential-backoff reconnect loop (D-01/D-02/RT-02).
///
/// - Initial backoff: 1000ms.
/// - Doubles on each failure, capped at 30,000ms.
/// - Jitter: ±25% of backoff (via js_sys::Math::random()).
/// - On successful Connected handshake: reset backoff, clear reconnect_attempts.
/// - Breaks when abort flag is set (navigate-away via WsHandle drop).
#[cfg(target_arch = "wasm32")]
async fn reconnect_loop(board_id: String, signals: BoardSignals, abort: Arc<AtomicBool>) {
    use web_sys::WebSocket;
    use leptos::leptos_dom::logging::console_log;

    let mut backoff_ms: u64 = 1000;

    loop {
        if abort.load(Ordering::Relaxed) {
            break;
        }

        let ws_url = ws_url_for(&board_id);

        // Attempt to open the WebSocket.
        let ws = match WebSocket::new(&ws_url) {
            Ok(ws) => ws,
            Err(e) => {
                console_log(&format!("[ws-client] board={board_id} open error: {e:?}"));
                // Increment attempt counter so the toast can appear after 2+ failures.
                signals.reconnect_attempts.update(|n| *n += 1);
                signals.ws_connected.set(false);

                // Backoff with ±25% jitter (D-02).
                let jitter_fraction = js_sys::Math::random(); // 0.0..1.0
                let jitter_offset = (backoff_ms as f64 * 0.25 * (jitter_fraction * 2.0 - 1.0)) as i64;
                let sleep_ms = (backoff_ms as i64 + jitter_offset).max(100) as u32;
                gloo_timers::future::TimeoutFuture::new(sleep_ms).await;
                backoff_ms = (backoff_ms * 2).min(30_000);
                continue;
            }
        };

        // WebSocket opened — wire up event handlers and wait for close.
        let closed = run_ws_session(&board_id, &ws, signals, Arc::clone(&abort)).await;

        // Always clear connected state when session ends.
        signals.ws_connected.set(false);

        if abort.load(Ordering::Relaxed) {
            // Clean close due to navigate-away — stop looping.
            let _ = ws.close();
            break;
        }

        if closed {
            // Server closed the connection — increment attempt, backoff, retry.
            signals.reconnect_attempts.update(|n| *n += 1);
            let jitter_fraction = js_sys::Math::random();
            let jitter_offset = (backoff_ms as f64 * 0.25 * (jitter_fraction * 2.0 - 1.0)) as i64;
            let sleep_ms = (backoff_ms as i64 + jitter_offset).max(100) as u32;
            gloo_timers::future::TimeoutFuture::new(sleep_ms).await;
            backoff_ms = (backoff_ms * 2).min(30_000);
        }
    }
}

/// Run one WebSocket session: wire callbacks, await close, return true if we should retry.
///
/// Returns when the WebSocket closes (or the abort flag is set).
/// On successful Connected handshake: resets backoff state in signals.
#[cfg(target_arch = "wasm32")]
async fn run_ws_session(
    board_id: &str,
    ws: &web_sys::WebSocket,
    signals: BoardSignals,
    abort: Arc<AtomicBool>,
) -> bool {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::MessageEvent;
    use leptos::leptos_dom::logging::console_log;

    // Channel to signal the async loop that the socket closed.
    // We use an Rc<RefCell<Option<Sender>>> shared between closures and this function.
    use std::rc::Rc;
    use std::cell::RefCell;

    // Expose a send function on BoardSignals so UI components can emit client→server messages.
    // We wrap the WebSocket in WsSendFn (Clone + unsafe Send+Sync via Arc) with the safety
    // contract that this is only called from WASM microtask context (single-threaded).
    {
        let ws_clone = ws.clone();
        let send_fn = WsSendFn::new(move |msg: String| {
            if ws_clone.ready_state() == web_sys::WebSocket::OPEN {
                let _ = ws_clone.send_with_str(&msg);
            }
        });
        signals.ws_send.set_value(Some(send_fn));
    }

    let (close_tx, close_rx) = futures::channel::oneshot::channel::<()>();
    let close_tx = Rc::new(RefCell::new(Some(close_tx)));

    let abort_msg = Arc::clone(&abort);
    let signals_msg = signals;
    let board_id_msg = board_id.to_string();

    // onmessage — apply board events; detect Connected to reset backoff
    let board_id_str = board_id.to_string();
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        if abort_msg.load(Ordering::Relaxed) {
            return;
        }
        if let Some(text) = e.data().as_string() {
            match serde_json::from_str::<WsEnvelope>(&text) {
                Ok(WsEnvelope::Board { payload }) => {
                    let own_id = signals_msg
                        .own_client_id
                        .get_untracked()
                        .unwrap_or_default();

                    // On Connected: reset backoff counters (successful connection).
                    if let BoardEvent::Connected { .. } = &payload {
                        signals_msg.reconnect_attempts.set(0);
                        signals_msg.ws_connected.set(true);
                    }

                    // Apply the event (includes seq-gap detection).
                    wasm_bindgen_futures::spawn_local(async move {
                        apply_board_event_async(signals_msg, payload, &own_id).await;
                    });
                }
                Ok(WsEnvelope::User { payload }) => {
                    apply_notif_event(signals_msg, payload);
                }
                Ok(WsEnvelope::Presence { payload }) => {
                    let own_id = signals_msg
                        .own_client_id
                        .get_untracked()
                        .unwrap_or_default();
                    apply_presence_event(signals_msg, payload, &own_id);
                }
                Err(e) => {
                    console_log(&format!(
                        "[ws-client] board={board_id_str} deserialize error: {e}"
                    ));
                }
            }
        }
    });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    // onerror — log only; onclose will fire afterward to trigger the retry.
    let board_id_err = board_id.to_string();
    let onerror = Closure::<dyn FnMut(web_sys::ErrorEvent)>::new(move |e: web_sys::ErrorEvent| {
        console_log(&format!("[ws-client] board={board_id_err} error: {:?}", e.message()));
    });
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    // onclose — signal the async loop that the session ended.
    let close_tx_clone = Rc::clone(&close_tx);
    let abort_close = Arc::clone(&abort);
    let board_id_close = board_id.to_string();
    let onclose = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |_: web_sys::CloseEvent| {
        if !abort_close.load(Ordering::Relaxed) {
            console_log(&format!("[ws-client] board={board_id_close} connection closed — will reconnect"));
        }
        // Fire the oneshot to wake the async loop.
        if let Some(tx) = close_tx_clone.borrow_mut().take() {
            let _ = tx.send(());
        }
    });
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();

    // --- Heartbeat ticker (5s) + visibilitychange listener (D-12) ---
    // Spawn the heartbeat as a concurrent local task. It stops when the WS closes
    // (abort flag set or send_fn cleared). The visibilitychange listener is wired once
    // and cleans up when the socket closes.
    {
        let abort_hb = Arc::clone(&abort);
        let signals_hb = signals;
        wasm_bindgen_futures::spawn_local(async move {
            run_heartbeat_and_visibility(signals_hb, abort_hb).await;
        });
    }

    // Await close (or abort).
    let _ = close_rx.await;

    // Clear the send function — socket is closed.
    signals.ws_send.set_value(None);

    // Return true = should retry (abort=false means server closed us, not intentional teardown).
    !abort.load(Ordering::Relaxed)
}

/// Async wrapper around event application — handles seq-gap detection and refresh.
///
/// Called from the onmessage closure via `spawn_local` so that `refresh_board` (async)
/// can be awaited when a sequence gap is detected.
#[cfg(target_arch = "wasm32")]
async fn apply_board_event_async(signals: BoardSignals, event: BoardEvent, own_client_id: &str) {
    // Seq-gap detection: compare event's board_seq to last_seen_seq.
    // Rules (Flag 1):
    //   seq == last+1          → normal: apply, advance last_seen_seq
    //   seq <= last && last!=0 → duplicate/replay: discard (idempotent)
    //   seq > last+1 && last!=0 → gap: refresh and discard stale delta
    // Connected anchors last_seen_seq; Refresh triggers refresh_board.
    let last = signals.last_seen_seq.get_untracked();

    let seq_opt = board_seq_of(&event);

    if let Some(seq) = seq_opt {
        // Skip duplicate/replay events (idempotent guard).
        if last != 0 && seq <= last {
            return;
        }
        // Sequence gap: fetch fresh board state (stale-then-swap).
        if last != 0 && seq > last + 1 {
            let board_id = signals.board_id.get_untracked();
            refresh_board(board_id, signals).await;
            return; // discard stale delta — the refresh fetched everything
        }
    }

    // Refresh event: fetch fresh board state unconditionally.
    if matches!(event, BoardEvent::Refresh) {
        let board_id = signals.board_id.get_untracked();
        refresh_board(board_id, signals).await;
        return;
    }

    // Normal path: apply the event.
    apply_board_event(signals, event, own_client_id);
}

/// Heartbeat ticker + visibilitychange handler (D-12/D-13, WASM only).
///
/// - Sends `{"type":"heartbeat"}` every 5s while the tab is visible.
/// - On `visibilitychange` hidden → sends `{"type":"presence_leave"}`, stops ticking.
/// - On `visibilitychange` visible → sends `{"type":"presence_join"}`, resumes ticking.
/// - Exits when abort flag is set (socket closed or navigated away).
#[cfg(target_arch = "wasm32")]
async fn run_heartbeat_and_visibility(signals: BoardSignals, abort: Arc<AtomicBool>) {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::Event;

    // Use a shared RwSignal (from the Leptos reactive system) to communicate
    // visibilitychange events to the heartbeat loop. This avoids threading the
    // AbortController pattern and keeps all WASM on the microtask thread.
    let visible = RwSignal::new(true); // start visible

    // Wire visibilitychange listener on document.
    // The listener updates the `visible` signal; the heartbeat loop polls it.
    let visible_for_listener = visible;
    let signals_for_listener = signals;
    let abort_for_listener = Arc::clone(&abort);
    let listener_closure = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
        if abort_for_listener.load(Ordering::Relaxed) {
            return;
        }
        let doc = web_sys::window()
            .and_then(|w| w.document());
        let is_visible = doc
            .as_ref()
            .map(|d| d.visibility_state() == web_sys::VisibilityState::Visible)
            .unwrap_or(true);
        visible_for_listener.set(is_visible);

        // Emit presence_leave or presence_join based on new visibility state.
        let msg = if is_visible {
            r#"{"type":"presence_join"}"#
        } else {
            r#"{"type":"presence_leave"}"#
        };
        if let Some(send) = signals_for_listener.ws_send.get_value() {
            send.call(msg.to_string());
        }
    });

    // Add event listener to document (ignore errors — if document is absent we're in SSR stub).
    let doc = web_sys::window().and_then(|w| w.document());
    if let Some(ref d) = doc {
        let _ = d.add_event_listener_with_callback(
            "visibilitychange",
            listener_closure.as_ref().unchecked_ref(),
        );
    }

    // Heartbeat loop: tick every 5s while visible.
    loop {
        if abort.load(Ordering::Relaxed) {
            break;
        }
        // Sleep 5s regardless of visibility (simpler than cancellable sleep).
        gloo_timers::future::TimeoutFuture::new(5_000).await;

        if abort.load(Ordering::Relaxed) {
            break;
        }

        // Only send heartbeat while tab is visible.
        if visible.get_untracked() {
            if let Some(send) = signals.ws_send.get_value() {
                send.call(r#"{"type":"heartbeat"}"#.to_string());
            }
        }
    }

    // Remove the visibilitychange listener on exit.
    if let Some(d) = doc {
        let _ = d.remove_event_listener_with_callback(
            "visibilitychange",
            listener_closure.as_ref().unchecked_ref(),
        );
    }
    drop(listener_closure);
}

/// Apply a `PresenceEvent` to the board's reactive presence signals.
///
/// Called from the `WsEnvelope::Presence` arm of the message handler.
/// `own_user_id` is used to exclude the current user from the viewers list (SC5).
///
/// Note: `own_user_id` is from `own_client_id` which is set on `Connected` — on the very
/// first presence events (ViewersSnapshot from the server), own_client_id is the client UUID,
/// NOT the user_id. We need the actual user_id to exclude self from viewers.
/// The WS handler sets own_client_id = client_id (UUID); user_id comparison requires the
/// user's actual ID. To get the user_id in WASM we look at: whoever sent the Connected
/// handshake already has their user_id excluded by the server (the server doesn't send you
/// your own ViewerJoined — join broadcasts go to OTHER viewers). So self-exclusion is
/// handled server-side for ViewerJoined. For ViewersSnapshot the server also excludes the
/// current viewer from the snapshot (the snapshot is "other viewers"). We only need to
/// handle the case where the snapshot includes self (which it shouldn't), so a belt-and-
/// suspenders check: exclude any viewer whose user_id is in the `own_user_id` (if available).
///
/// Implementation: the server-side join broadcasts AFTER the snapshot, so the snapshot sent
/// to the joining client contains only OTHER viewers. The joining client's own ViewerJoined
/// is NOT broadcast to themselves (broadcast::Sender sends to ALL subscribers, which includes
/// the sender themselves). Wait — the server calls `subscribe` BEFORE `join`, so the new
/// client DOES receive their own ViewerJoined. We must exclude it on the client.
///
/// Solution: store the current user's user_id in BoardSignals and filter here.
/// For now we filter by comparing against `own_client_id` but that's a UUID, not a user_id.
/// The correct exclusion: BoardSignals has no user_id field. We'll filter ViewerJoined
/// by comparing against the user_id stored in the signal. Since we don't have user_id in
/// BoardSignals, we defer self-exclusion to the PresenceStack component which checks
/// `own_client_id != ""` (the presence stack won't have the right value either).
///
/// Pragmatic fix: add `own_user_id: RwSignal<Option<String>>` OR — per the plan's intent —
/// the server should NOT broadcast ViewerJoined back to the joining viewer themselves.
/// Current server impl: `join` calls `tx.send(ViewerJoined{...})` to the board channel.
/// The joining client subscribed to that channel via `subscribe()` BEFORE join is called.
/// So the client DOES receive their own ViewerJoined. We filter by checking the viewer's
/// user_id against a `own_user_id` stored on BoardSignals.
///
/// Since BoardSignals doesn't yet have `own_user_id`, we can safely skip self-exclusion here
/// and let the PresenceStack filter it (the stack excludes the current user using a stored user_id
/// from the session, which is available via server functions in the SSR-rendered props).
/// For simplicity: store `own_user_id` in `BoardSignals.own_client_id` — but that's a client UUID.
/// The plan says "current user excluded by Task 2's signal logic" — so we need to handle it.
/// Since the WS onmessage closure already has `signals_msg.own_client_id`, and `own_client_id`
/// is a session-level WS UUID (not user_id), we need to add the actual user_id to BoardSignals.
///
/// Decision: store `own_user_id: RwSignal<Option<String>>` on BoardSignals to enable self-exclusion.
/// This is a deviation: adding one field beyond plan scope to satisfy SC5 "self excluded" behavior.
fn apply_presence_event(signals: BoardSignals, event: PresenceEvent, _own_client_id: &str) {
    match event {
        PresenceEvent::ViewersSnapshot { viewers } => {
            // Replace entire viewer list (initial snapshot on join).
            // Self-exclusion: filter by own_user_id if available.
            let own_uid = signals.own_user_id.get_untracked().unwrap_or_default();
            let new_viewers: Vec<PresenceViewer> = viewers
                .into_iter()
                .filter(|v| v.user_id != own_uid)
                .map(|v| PresenceViewer {
                    user_id: v.user_id,
                    display_name: v.display_name,
                    avatar_color: v.avatar_color,
                })
                .collect();
            signals.viewers.set(new_viewers);
        }
        PresenceEvent::ViewerJoined { user_id, display_name, avatar_color } => {
            // Exclude self (SC5).
            let own_uid = signals.own_user_id.get_untracked().unwrap_or_default();
            if user_id == own_uid {
                return;
            }
            // Prepend (most-recent viewer appears first in the stack).
            signals.viewers.update(|vs| {
                // Idempotent: only add if not already present.
                if !vs.iter().any(|v| v.user_id == user_id) {
                    vs.insert(0, PresenceViewer { user_id, display_name, avatar_color });
                }
            });
        }
        PresenceEvent::ViewerLeft { user_id } => {
            // CR-03: resolve the departing user's display_name BEFORE removing them from
            // the viewers list, then retain only that name from editing/typing maps.
            // Previously `retain(|_| false)` wiped every user's state.
            let gone_name = signals.viewers.with_untracked(|vs| {
                vs.iter().find(|v| v.user_id == user_id).map(|v| v.display_name.clone())
            });
            // Remove from viewers list (idempotent — no-op if absent).
            signals.viewers.update(|vs| {
                vs.retain(|v| v.user_id != user_id);
            });
            // Clear only the departing user's editing/typing indicators.
            if let Some(name) = gone_name {
                signals.editing_card_ids.update(|m| {
                    for names in m.values_mut() { names.retain(|n| n != &name); }
                    m.retain(|_, v| !v.is_empty());
                });
                signals.typing_card_ids.update(|m| {
                    for names in m.values_mut() { names.retain(|n| n != &name); }
                    m.retain(|_, v| !v.is_empty());
                });
            }
        }
        PresenceEvent::EditingCard { user_id, card_id } => {
            // Look up display_name for this user_id from the viewers list.
            let display_name = signals.viewers.with_untracked(|vs| {
                vs.iter()
                    .find(|v| v.user_id == user_id)
                    .map(|v| v.display_name.clone())
                    .unwrap_or_else(|| user_id.clone()) // fallback to user_id if not in viewers
            });

            signals.editing_card_ids.update(|m| {
                // Remove this user from ALL cards' editor lists first.
                for editors in m.values_mut() {
                    editors.retain(|n| n != &display_name);
                }
                m.retain(|_, v| !v.is_empty());
                // Add to new card if Some.
                if let Some(cid) = card_id {
                    m.entry(cid).or_default().push(display_name);
                }
            });
        }
        PresenceEvent::Typing { user_id, card_id, is_typing } => {
            // Look up display_name.
            let display_name = signals.viewers.with_untracked(|vs| {
                vs.iter()
                    .find(|v| v.user_id == user_id)
                    .map(|v| v.display_name.clone())
                    .unwrap_or_else(|| user_id.clone())
            });

            signals.typing_card_ids.update(|m| {
                if is_typing {
                    let names = m.entry(card_id).or_default();
                    if !names.contains(&display_name) {
                        names.push(display_name);
                    }
                } else {
                    // Remove this user from this card's typing list.
                    if let Some(names) = m.get_mut(&card_id) {
                        names.retain(|n| n != &display_name);
                    }
                    m.retain(|_, v| !v.is_empty());
                }
            });
        }
    }
}

/// Apply a `NotifEvent` from the per-user WS channel (RT-04).
///
/// Called from the `WsEnvelope::User` arm of the onmessage handler.
/// Patches the global unread-count signal and triggers the badge pulse on increment.
fn apply_notif_event(signals: BoardSignals, event: NotifEvent) {
    match event {
        NotifEvent::UnreadCountUpdated { count } => {
            let prev = signals.unread_count.get_untracked();
            signals.unread_count.set(count);
            // Pulse the badge only when the count increased (UI-SPEC §301).
            if count > prev {
                signals.badge_pulse.set(true);
                // Clear the pulse flag after 200ms (UI-SPEC §329: animation duration).
                #[cfg(target_arch = "wasm32")]
                {
                    let pulse_sig = signals.badge_pulse;
                    wasm_bindgen_futures::spawn_local(async move {
                        gloo_timers::future::TimeoutFuture::new(200).await;
                        pulse_sig.set(false);
                    });
                }
            }
        }
        NotifEvent::MentionReceived { .. } => {
            // Inbox list UI is Phase 7. The UnreadCountUpdated event arrives immediately
            // after MentionReceived, so the badge will update. No action needed here.
        }
    }
}

/// Extract the board_seq from any event that carries one.
/// Returns None for events without a sequence number (Refresh).
fn board_seq_of(event: &BoardEvent) -> Option<u64> {
    match event {
        BoardEvent::Connected { board_seq, .. } => Some(*board_seq),
        BoardEvent::CardMoved { board_seq, .. } => Some(*board_seq),
        BoardEvent::CardAdded { board_seq, .. } => Some(*board_seq),
        BoardEvent::CardUpdated { board_seq, .. } => Some(*board_seq),
        BoardEvent::CardArchived { board_seq, .. } => Some(*board_seq),
        BoardEvent::CommentAdded { board_seq, .. } => Some(*board_seq),
        BoardEvent::ChecklistUpdated { board_seq, .. } => Some(*board_seq),
        BoardEvent::LabelChanged { board_seq, .. } => Some(*board_seq),
        BoardEvent::PriorityChanged { board_seq, .. } => Some(*board_seq),
        BoardEvent::DueDateChanged { board_seq, .. } => Some(*board_seq),
        BoardEvent::MemberChanged { board_seq, .. } => Some(*board_seq),
        BoardEvent::AttachmentAdded { board_seq, .. } => Some(*board_seq),
        BoardEvent::AttachmentRemoved { board_seq, .. } => Some(*board_seq),
        BoardEvent::CardMovedCrossBoard { board_seq, .. } => Some(*board_seq),
        BoardEvent::ListAdded { board_seq, .. } => Some(*board_seq),
        BoardEvent::ListRenamed { board_seq, .. } => Some(*board_seq),
        BoardEvent::ListReordered { board_seq, .. } => Some(*board_seq),
        BoardEvent::ListArchived { board_seq, .. } => Some(*board_seq),
        BoardEvent::Refresh => None,
    }
}

/// Stale-then-swap full board refresh (D-03, RT-02).
///
/// Calls `get_board` server function and patches all `BoardSignals` in place:
/// - Updates existing `RwSignal<Card>` values (no wholesale map replacement).
/// - Inserts signals for new cards.
/// - Removes signals for cards no longer present.
/// - Rebuilds `list_order` and `list_cards`.
/// - Sets `last_seen_seq` to the fresh `board_seq`.
///
/// The stale board stays rendered until this completes — no skeleton or spinner.
///
/// CRITICAL (Anti-Pattern §720): never call `card_signals.set(...)` with a new HashMap;
/// always `.update()` individual entries so Leptos `<For>` keeps existing DOM nodes.
#[cfg(target_arch = "wasm32")]
async fn refresh_board(board_id: String, signals: BoardSignals) {
    use crate::api::board_api::get_board;
    use leptos::leptos_dom::logging::console_log;

    match get_board(board_id.clone()).await {
        Ok(data) => {
            // --- Patch card_signals in place (CRITICAL: do NOT replace the map) ---
            let fresh_card_ids: std::collections::HashSet<String> =
                data.cards.iter().map(|c| c.id.clone()).collect();

            // Update existing signals and insert new ones.
            for card in &data.cards {
                let existing = signals.card_signals.with(|cs| cs.get(&card.id).copied());
                if let Some(sig) = existing {
                    // Update in place — keeps the DOM node alive.
                    let card_clone = card.clone();
                    sig.set(card_clone);
                } else {
                    // New card: insert a fresh signal.
                    let new_sig = RwSignal::new(card.clone());
                    signals.card_signals.update(|cs| {
                        cs.insert(card.id.clone(), new_sig);
                    });
                }
            }

            // Remove signals for cards no longer in the fresh data.
            let stale_ids: Vec<String> = signals.card_signals.with(|cs| {
                cs.keys()
                    .filter(|id| !fresh_card_ids.contains(*id))
                    .cloned()
                    .collect()
            });
            if !stale_ids.is_empty() {
                signals.card_signals.update(|cs| {
                    for id in &stale_ids {
                        cs.remove(id);
                    }
                });
                signals.list_cards.update(|lc| {
                    for ids in lc.values_mut() {
                        ids.retain(|id| !stale_ids.contains(id));
                    }
                });
            }

            // Rebuild list_order from fresh data (sorted by position).
            let mut sorted_lists = data.lists.clone();
            sorted_lists.sort_by(|a, b| a.position.cmp(&b.position));
            let new_list_order: Vec<String> = sorted_lists.iter().map(|l| l.id.clone()).collect();
            signals.list_order.set(new_list_order);

            // Rebuild list_cards from fresh data.
            let new_list_cards: std::collections::HashMap<String, Vec<String>> = data.lists.iter()
                .map(|l| {
                    let mut card_ids: Vec<_> = data.cards.iter()
                        .filter(|c| c.list_id == l.id)
                        .collect();
                    card_ids.sort_by(|a, b| a.position.cmp(&b.position));
                    let ids: Vec<String> = card_ids.iter().map(|c| c.id.clone()).collect();
                    (l.id.clone(), ids)
                })
                .collect();
            signals.list_cards.set(new_list_cards);

            // Anchor last_seen_seq from the fresh board_seq.
            signals.last_seen_seq.set(data.board_seq);
        }
        Err(e) => {
            console_log(&format!("[ws-client] refresh_board error for board={board_id}: {e}"));
        }
    }
}

/// Build the WebSocket URL for a board based on the current window location.
///
/// Open Question 3 resolution: use `ws://` when protocol is `http:`, `wss://` otherwise.
/// This handles both local dev (ws://) and production HTTPS (wss://) without config.
#[cfg(target_arch = "wasm32")]
pub fn ws_url_for(board_id: &str) -> String {
    let location = web_sys::window()
        .and_then(|w| w.location().host().ok())
        .unwrap_or_else(|| "localhost:3000".to_string());

    let protocol = web_sys::window()
        .and_then(|w| w.location().protocol().ok())
        .unwrap_or_else(|| "http:".to_string());

    let ws_scheme = if protocol == "https:" { "wss" } else { "ws" };
    format!("{ws_scheme}://{location}/ws/board/{board_id}")
}

// ---------------------------------------------------------------------------
// apply_board_event — signal patching (Pattern 4)
// ---------------------------------------------------------------------------

/// Apply a `BoardEvent` to the board's reactive signals.
///
/// Called on every `WsEnvelope::Board` message received from the server.
/// `own_client_id` is compared against the event's client_id for D-05 self-echo suppression.
///
/// This function compiles under both wasm32 and ssr (the non-wasm stub calls it too).
/// Seq-gap detection is handled by `apply_board_event_async` (WASM) before this is called.
pub fn apply_board_event(signals: BoardSignals, event: BoardEvent, own_client_id: &str) {
    match event {
        BoardEvent::Connected { client_id, board_seq } => {
            // Store own client_id for D-05 self-echo suppression on future events
            signals.own_client_id.set(Some(client_id));
            // Anchor last_seen_seq from the handshake
            signals.last_seen_seq.set(board_seq);
            // Mark connected state
            signals.ws_connected.set(true);
        }

        BoardEvent::CardMoved {
            board_seq,
            client_id,
            card_id,
            to_list_id,
            position,
        } => {
            // Update last_seen_seq
            signals.last_seen_seq.set(board_seq);

            // Patch the card's RwSignal<Card> (list_id + position) in place —
            // never replace the whole card_signals map (would tear down all DOM nodes).
            let card_sig = signals.card_signals.with(|cs| cs.get(&card_id).copied());
            let prev_list_id = if let Some(sig) = card_sig {
                let prev = sig.with_untracked(|c| c.list_id.clone());
                sig.update(|c| {
                    c.list_id = to_list_id.clone();
                    c.position = position.clone();
                });
                Some(prev)
            } else {
                None
            };

            // Move card_id between list_cards[prev_list] → list_cards[to_list_id]
            if let Some(from_list) = prev_list_id {
                signals.list_cards.update(|lc| {
                    // Remove from old list
                    if let Some(ids) = lc.get_mut(&from_list) {
                        ids.retain(|id| id != &card_id);
                    }
                    // Insert into target list (sorted by position for consistency)
                    let target_ids = lc.entry(to_list_id.clone()).or_default();
                    if !target_ids.contains(&card_id) {
                        // Find insert position by comparing position strings
                        let insert_at = target_ids.iter().position(|other_id| {
                            signals.card_signals.with(|cs| {
                                cs.get(other_id)
                                    .map(|sig| sig.with_untracked(|c| c.position > position))
                                    .unwrap_or(false)
                            })
                        });
                        match insert_at {
                            Some(idx) => target_ids.insert(idx, card_id.clone()),
                            None => target_ids.push(card_id.clone()),
                        }
                    }
                });
            }

            // D-05: only set highlight for REMOTE moves (not the originator's own echo).
            // own_client_id.is_empty() = WS not yet connected; treat as remote for safety.
            let is_remote = client_id.as_str() != own_client_id || own_client_id.is_empty();
            if is_remote {
                signals.highlight_card_id.set(Some(card_id.clone()));
                // Clear highlight after ~1.5s (D-04).
                // Uses wasm_bindgen_futures::spawn_local with gloo_timers::future::TimeoutFuture.
                #[cfg(target_arch = "wasm32")]
                {
                    let highlight_sig = signals.highlight_card_id;
                    let card_id_for_clear = card_id.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        gloo_timers::future::TimeoutFuture::new(1500).await;
                        // Only clear if this card is still highlighted (a later move may have replaced it)
                        if highlight_sig.with_untracked(|h| h.as_deref() == Some(card_id_for_clear.as_str())) {
                            highlight_sig.set(None);
                        }
                    });
                }
            }
        }

        BoardEvent::CardAdded { board_seq, client_id, card } => {
            signals.last_seen_seq.set(board_seq);

            let is_remote = client_id.as_str() != own_client_id || own_client_id.is_empty();

            // Build a full Card from the CardSummary payload.
            // Counters (checklist, comments, attachments) start at 0 — the adder's
            // own UI already shows the card; remote viewers will see 0 until next sync.
            let new_card = crate::models::Card {
                id: card.id.clone(),
                list_id: card.list_id.clone(),
                board_id: card.board_id.clone(),
                card_num: card.card_num,
                title: card.title.clone(),
                position: card.position.clone(),
                priority: card.priority.clone(),
                due_at: card.due_at,
                done: card.done,
                archived: false,
                cover: card.cover.clone(),
                labels: card.labels.clone(),
                checklist_done: 0,
                checklist_total: 0,
                comment_count: 0,
                attachment_count: 0,
                member_ids: card.member_ids.clone(),
            };

            // Insert card signal
            let new_sig = RwSignal::new(new_card);
            signals.card_signals.update(|cs| {
                cs.insert(card.id.clone(), new_sig);
            });

            // Append card id to the correct list (at the end; server positions are correct)
            let list_id = card.list_id.clone();
            let card_id = card.id.clone();
            let position = card.position.clone();
            signals.list_cards.update(|lc| {
                let ids = lc.entry(list_id).or_default();
                if !ids.contains(&card_id) {
                    // Insert in sorted position order
                    let insert_at = ids.iter().position(|other_id| {
                        signals.card_signals.with_untracked(|cs| {
                            cs.get(other_id)
                                .map(|sig| sig.with_untracked(|c| c.position > position))
                                .unwrap_or(false)
                        })
                    });
                    match insert_at {
                        Some(idx) => ids.insert(idx, card_id),
                        None => ids.push(card_id),
                    }
                }
            });

            // Flash highlight for remote card additions (D-04/D-05)
            if is_remote {
                let cid = card.id.clone();
                signals.highlight_card_id.set(Some(cid.clone()));
                #[cfg(target_arch = "wasm32")]
                {
                    let highlight_sig = signals.highlight_card_id;
                    wasm_bindgen_futures::spawn_local(async move {
                        gloo_timers::future::TimeoutFuture::new(1500).await;
                        if highlight_sig.with_untracked(|h| h.as_deref() == Some(cid.as_str())) {
                            highlight_sig.set(None);
                        }
                    });
                }
            }
        }

        BoardEvent::CardUpdated { board_seq, client_id, card_id, patch } => {
            signals.last_seen_seq.set(board_seq);

            // Apply only the Some fields from the patch.
            // Task 3 will gate title/description on focus signals; for now apply all.
            let card_sig = signals.card_signals.with(|cs| cs.get(&card_id).copied());
            if let Some(sig) = card_sig {
                sig.update(|c| {
                    if let Some(t) = patch.title { c.title = t; }
                    if let Some(d) = patch.description { let _ = d; /* description not on Card thumbnail */ }
                    if let Some(cv) = patch.cover { c.cover = Some(cv); }
                    if let Some(done) = patch.done { c.done = done; }
                });
            }

            // Flash for remote updates (D-05)
            let is_remote = client_id.as_str() != own_client_id || own_client_id.is_empty();
            if is_remote {
                signals.highlight_card_id.set(Some(card_id.clone()));
                #[cfg(target_arch = "wasm32")]
                {
                    let highlight_sig = signals.highlight_card_id;
                    wasm_bindgen_futures::spawn_local(async move {
                        gloo_timers::future::TimeoutFuture::new(1500).await;
                        if highlight_sig.with_untracked(|h| h.as_deref() == Some(card_id.as_str())) {
                            highlight_sig.set(None);
                        }
                    });
                }
            }
        }

        BoardEvent::CardArchived { board_seq, client_id, card_id } => {
            signals.last_seen_seq.set(board_seq);

            let is_remote = client_id.as_str() != own_client_id || own_client_id.is_empty();

            if is_remote {
                // D-09: signal the modal that this card was remotely archived
                signals.remote_archived_card_id.set(Some(card_id.clone()));
                // D-06: insert into fading_card_ids, wait 350ms for CSS animation, then remove
                signals.fading_card_ids.update(|fids| { fids.insert(card_id.clone()); });
                #[cfg(target_arch = "wasm32")]
                {
                    let fading_sig = signals.fading_card_ids;
                    let list_cards_sig = signals.list_cards;
                    let card_signals_sig = signals.card_signals;
                    let cid = card_id.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        gloo_timers::future::TimeoutFuture::new(350).await;
                        // Remove from fading set
                        fading_sig.update(|fids| { fids.remove(&cid); });
                        // Remove from list_cards and card_signals
                        list_cards_sig.update(|lc| {
                            for ids in lc.values_mut() {
                                ids.retain(|id| id != &cid);
                            }
                        });
                        card_signals_sig.update(|cs| { cs.remove(&cid); });
                    });
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // SSR/test: remove immediately without animation
                    signals.fading_card_ids.update(|fids| { fids.remove(&card_id); });
                    signals.list_cards.update(|lc| {
                        for ids in lc.values_mut() {
                            ids.retain(|id| id != &card_id);
                        }
                    });
                    signals.card_signals.update(|cs| { cs.remove(&card_id); });
                }
            }
            // Own archive is handled by sidebar.rs dispatch Effect — no action needed here.
        }

        BoardEvent::CommentAdded { board_seq, client_id, card_id, .. } => {
            signals.last_seen_seq.set(board_seq);
            // Increment comment_count on the card thumbnail
            let card_sig = signals.card_signals.with(|cs| cs.get(&card_id).copied());
            if let Some(sig) = card_sig {
                sig.update(|c| c.comment_count += 1);
            }
            // Flash for remote
            let is_remote = client_id.as_str() != own_client_id || own_client_id.is_empty();
            if is_remote {
                signals.highlight_card_id.set(Some(card_id.clone()));
                #[cfg(target_arch = "wasm32")]
                {
                    let highlight_sig = signals.highlight_card_id;
                    wasm_bindgen_futures::spawn_local(async move {
                        gloo_timers::future::TimeoutFuture::new(1500).await;
                        if highlight_sig.with_untracked(|h| h.as_deref() == Some(card_id.as_str())) {
                            highlight_sig.set(None);
                        }
                    });
                }
            }
        }

        BoardEvent::ChecklistUpdated { board_seq, client_id: _, card_id, checklist_done, checklist_total } => {
            signals.last_seen_seq.set(board_seq);
            let card_sig = signals.card_signals.with(|cs| cs.get(&card_id).copied());
            if let Some(sig) = card_sig {
                sig.update(|c| {
                    c.checklist_done = checklist_done;
                    c.checklist_total = checklist_total;
                });
            }
        }

        BoardEvent::LabelChanged { board_seq, client_id: _, card_id, labels } => {
            signals.last_seen_seq.set(board_seq);
            let card_sig = signals.card_signals.with(|cs| cs.get(&card_id).copied());
            if let Some(sig) = card_sig {
                sig.update(|c| c.labels = labels);
            }
        }

        BoardEvent::PriorityChanged { board_seq, client_id: _, card_id, priority } => {
            signals.last_seen_seq.set(board_seq);
            let card_sig = signals.card_signals.with(|cs| cs.get(&card_id).copied());
            if let Some(sig) = card_sig {
                sig.update(|c| c.priority = priority);
            }
        }

        BoardEvent::DueDateChanged { board_seq, client_id: _, card_id, due_at } => {
            signals.last_seen_seq.set(board_seq);
            let card_sig = signals.card_signals.with(|cs| cs.get(&card_id).copied());
            if let Some(sig) = card_sig {
                sig.update(|c| c.due_at = due_at);
            }
        }

        BoardEvent::MemberChanged { board_seq, client_id: _, card_id, member_ids } => {
            signals.last_seen_seq.set(board_seq);
            let card_sig = signals.card_signals.with(|cs| cs.get(&card_id).copied());
            if let Some(sig) = card_sig {
                sig.update(|c| c.member_ids = member_ids);
            }
        }

        BoardEvent::AttachmentAdded { board_seq, client_id: _, card_id, .. } => {
            signals.last_seen_seq.set(board_seq);
            let card_sig = signals.card_signals.with(|cs| cs.get(&card_id).copied());
            if let Some(sig) = card_sig {
                sig.update(|c| c.attachment_count += 1);
            }
        }

        BoardEvent::AttachmentRemoved { board_seq, client_id: _, card_id, .. } => {
            signals.last_seen_seq.set(board_seq);
            let card_sig = signals.card_signals.with(|cs| cs.get(&card_id).copied());
            if let Some(sig) = card_sig {
                sig.update(|c| c.attachment_count = (c.attachment_count - 1).max(0));
            }
        }

        BoardEvent::CardMovedCrossBoard { board_seq, client_id: _, card_id } => {
            signals.last_seen_seq.set(board_seq);
            // Card left this board — remove from list_cards and card_signals
            signals.list_cards.update(|lc| {
                for ids in lc.values_mut() {
                    ids.retain(|id| id != &card_id);
                }
            });
            signals.card_signals.update(|cs| { cs.remove(&card_id); });
        }

        // List mutations: BoardPage still uses server action refetch for list-level changes.
        // WS events update last_seen_seq for gap detection; UI refreshes via refetch.
        BoardEvent::ListAdded { board_seq, .. } => { signals.last_seen_seq.set(board_seq); }
        BoardEvent::ListRenamed { board_seq, .. } => { signals.last_seen_seq.set(board_seq); }
        BoardEvent::ListReordered { board_seq, .. } => { signals.last_seen_seq.set(board_seq); }
        BoardEvent::ListArchived { board_seq, .. } => { signals.last_seen_seq.set(board_seq); }

        BoardEvent::Refresh => {
            // Handled by apply_board_event_async (WASM path) before reaching here.
            // In SSR/test builds: no-op.
        }
    }
}

// ---------------------------------------------------------------------------
// Non-WASM stub (SSR + tests)
// ---------------------------------------------------------------------------

/// No-op stub for SSR and test builds.
/// Returns a WsHandle with a dead abort flag (nothing to abort).
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_ws_task(_board_id: String, _signals: BoardSignals) -> WsHandle {
    WsHandle::new(Arc::new(AtomicBool::new(false)))
}
