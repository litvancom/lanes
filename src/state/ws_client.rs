//! WASM WebSocket client task for per-board realtime sync (RT-01).
//!
//! `spawn_ws_task` opens a WebSocket connection to `/ws/board/:id`, receives
//! `WsEnvelope` JSON messages, and patches the board's reactive signals in place.
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
use crate::routes::board::BoardSignals;
use crate::models::events::{BoardEvent, WsEnvelope};
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
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::{MessageEvent, WebSocket};
    use leptos::leptos_dom::logging::console_log;

    let abort = Arc::new(AtomicBool::new(false));
    let handle = WsHandle::new(Arc::clone(&abort));

    let ws_url = ws_url_for(&board_id);

    // Open the WebSocket — errors here (bad URL, no network) are logged; task will retry if needed.
    let ws = match WebSocket::new(&ws_url) {
        Ok(ws) => ws,
        Err(e) => {
            console_log(&format!("WS open error for board {board_id}: {e:?}"));
            return handle;
        }
    };

    // Clone references for closures
    let abort_onmessage = Arc::clone(&abort);
    let signals_msg = signals;
    let board_id_msg = board_id.clone();

    // onmessage handler
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        if abort_onmessage.load(Ordering::Relaxed) {
            return;
        }
        if let Some(text) = e.data().as_string() {
            match serde_json::from_str::<WsEnvelope>(&text) {
                Ok(WsEnvelope::Board { payload }) => {
                    let own_id = signals_msg
                        .own_client_id
                        .get_untracked()
                        .unwrap_or_default();
                    apply_board_event(signals_msg, payload, &own_id);
                }
                Ok(WsEnvelope::User { .. }) => {
                    // 06-05: notification badge patching
                }
                Ok(WsEnvelope::Presence { .. }) => {
                    // 06-04: presence viewer list patching
                }
                Err(e) => {
                    console_log(&format!(
                        "[ws-client] board={board_id_msg} deserialize error: {e}"
                    ));
                }
            }
        }
    });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget(); // keep closure alive

    // onerror handler (log only; reconnect is future work in 06-03)
    let board_id_err = board_id.clone();
    let onerror = Closure::<dyn FnMut(web_sys::ErrorEvent)>::new(move |e: web_sys::ErrorEvent| {
        console_log(&format!("[ws-client] board={board_id_err} error: {:?}", e.message()));
    });
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    // onclose handler (log; reconnect in 06-03)
    let board_id_close = board_id.clone();
    let abort_onclose = Arc::clone(&abort);
    let onclose = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |_: web_sys::CloseEvent| {
        if !abort_onclose.load(Ordering::Relaxed) {
            console_log(&format!("[ws-client] board={board_id_close} closed (reconnect in 06-03)"));
        }
    });
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();

    // Store the WebSocket on the handle so we can close it on teardown.
    // For now, abort flag is the primary teardown signal.
    // The WsHandle::drop signals abort; the next onmessage will see it and stop processing.
    // In 06-03, we will also call ws.close() from a stored reference.
    let _ = ws; // ws is not stored; onerror/onclose handle lifecycle

    handle
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
pub fn apply_board_event(signals: BoardSignals, event: BoardEvent, own_client_id: &str) {
    match event {
        BoardEvent::Connected { client_id, board_seq } => {
            // Store own client_id for D-05 self-echo suppression on future events
            signals.own_client_id.set(Some(client_id));
            // Anchor last_seen_seq from the handshake
            signals.last_seen_seq.set(board_seq);
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
            // 06-03: trigger board_data.refetch() via a signal
            // Stub: no-op for now
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
