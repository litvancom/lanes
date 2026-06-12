//! Archive page: `/archive`
//!
//! Lists the current user's archived boards with muted styling.
//! Each archived board card shows a Restore button (non-destructive, no confirm) and
//! an owner-only Delete button (browser confirm required — permanent action, T-03-31).
//!
//! Auth guard: unauthenticated users are redirected to /login.
//!
//! # Security
//! - T-03-27: DeleteBoard server fn enforces owner-only role check server-side
//! - T-03-28: ArchiveBoard / RestoreBoard are owner-only server-side (03-01)
//! - T-03-29: list_archived_boards scopes results to the current user's boards
//! - T-03-31: permanent delete requires browser confirm() before dispatch

use leptos::prelude::*;
use leptos::web_sys;
use leptos_router::components::Redirect;
use crate::api::workspace_api::{
    list_archived_boards, RestoreBoard, DeleteBoard,
    list_boards_with_meta, list_starred_boards, ToggleStarBoard,
};
use crate::api::auth_api::get_current_user;
use crate::components::sidebar::WorkspaceSidebar;
use crate::components::topbar::WorkspaceTopbar;
use crate::components::icon::Icon;
use crate::models::BoardWithMeta;
use crate::components::board_card::safe_hex;
use crate::state::ws_client::spawn_notif_task;

/// A muted archived board card with Restore and owner-only Delete actions.
///
/// Design spec (UI-SPEC § ArchivePage BoardCard variant):
/// - opacity: 0.6 on the whole card
/// - color band: filter grayscale(0.7)
/// - Restore button: `.lns-btn.sm` (no confirm — non-destructive)
/// - Delete button: `.lns-btn.sm` with color: var(--danger), browser confirm first (T-03-31)
/// - No hover star
#[component]
fn ArchivedBoardCard(
    board: BoardWithMeta,
    restore_action: ServerAction<RestoreBoard>,
    delete_action: ServerAction<DeleteBoard>,
) -> impl IntoView {
    let c = safe_hex(&board.color);
    let gradient = format!("linear-gradient(135deg, {c}33, {c}11)");

    let board_id_restore = board.id.clone();
    let board_id_delete = board.id.clone();
    let board_name = board.name.clone();
    let board_name_confirm = board.name.clone();
    let card_count = board.card_count;

    view! {
        <div class="board-card board-card--archived">
            // Header band — grayscale muted (UI-SPEC)
            <div class="board-card-header-wrap">
                <div class="board-card-header board-card-header--archived" style=gradient/>
            </div>

            // Body
            <div class="board-card-body">
                <p class="board-card-name">{board_name}</p>
                <p class="board-card-meta">
                    {format!("{} card{}", card_count, if card_count == 1 { "" } else { "s" })}
                </p>

                // Archive action buttons
                <div class="board-card-archive-actions">
                    // Restore button — non-destructive, no confirm (UI-SPEC)
                    <button
                        type="button"
                        class="lns-btn lns-btn--sm"
                        on:click=move |_| {
                            restore_action.dispatch(RestoreBoard {
                                board_id: board_id_restore.clone(),
                            });
                        }
                    >
                        "Restore"
                    </button>

                    // Delete button — permanent; browser confirm required (T-03-31, UI-SPEC)
                    // Event handlers run only on the client (WASM), so window() is always available
                    <button
                        type="button"
                        class="lns-btn lns-btn--sm lns-btn--danger"
                        on:click=move |_| {
                            // Browser confirm dialog before destructive action (T-03-31)
                            // "Permanently delete this board? This cannot be undone."
                            let confirmed = web_sys::window()
                                .and_then(|w: web_sys::Window| {
                                    w.confirm_with_message(
                                        "Permanently delete this board? This cannot be undone."
                                    ).ok()
                                })
                                .unwrap_or(false);
                            if confirmed {
                                delete_action.dispatch(DeleteBoard {
                                    board_id: board_id_delete.clone(),
                                });
                            }
                        }
                    >
                        {format!("Delete \"{}\"", board_name_confirm)}
                    </button>
                </div>
            </div>
        </div>
    }
}

/// Archive page component (`/archive`).
///
/// Renders the user's archived boards in a muted grid.
/// Uses the same sidebar+main shell as workspace home for consistency.
///
/// Flow:
/// 1. Auth guard: get_current_user → Redirect /login if None
/// 2. Resource: list_archived_boards fetches archived boards for current user
/// 3. ServerAction<RestoreBoard> + ServerAction<DeleteBoard> — Effect refetches on success
/// 4. Renders `.lns-archive-grid` of ArchivedBoardCard components
///
/// Threat mitigations:
/// - T-03-29: list_archived_boards is scoped to current user
/// - T-03-27/28: delete/restore are owner-only server-side
/// - T-03-31: Delete requires browser confirm() dialog
#[component]
pub fn ArchivePage() -> impl IntoView {
    // ── Auth guard ─────────────────────────────────────────────────────────
    let current_user = Resource::new(|| (), |_| async { get_current_user().await });

    view! {
        <Suspense fallback=|| ()>
            {move || current_user.get().map(|result| match result {
                Ok(None) => view! {
                    <Redirect path="/login"/>
                }.into_any(),
                Err(_) => view! {
                    <div class="workspace-page">
                        <p class="board-error">"Something went wrong determining your session."</p>
                        <button
                            type="button"
                            class="lns-btn"
                            on:click=move |_| current_user.refetch()
                        >
                            "Retry"
                        </button>
                    </div>
                }.into_any(),
                Ok(Some(user)) => {
                    // ── Resources and actions ──────────────────────────────────────────
                    let archived = Resource::new(|| (), |_| async {
                        list_archived_boards().await
                    });

                    // Sidebar board data — mirrors workspace.rs pattern (WR-04)
                    let boards = Resource::new(|| (), |_| async { list_boards_with_meta().await });
                    let starred = Resource::new(|| (), |_| async { list_starred_boards().await });

                    let restore_action = ServerAction::<RestoreBoard>::new();
                    let delete_action = ServerAction::<DeleteBoard>::new();

                    // Star action for sidebar — refetches boards/starred on success
                    let star_action = ServerAction::<ToggleStarBoard>::new();
                    Effect::new(move |_| {
                        if matches!(star_action.value().get(), Some(Ok(_))) {
                            boards.refetch();
                            starred.refetch();
                        }
                    });
                    let star_cb: Callback<String> = Callback::new(move |board_id: String| {
                        star_action.dispatch(ToggleStarBoard { board_id });
                    });

                    // Refetch archived list when restore or delete succeeds
                    Effect::new(move |_| {
                        if matches!(restore_action.value().get(), Some(Ok(_))) {
                            archived.refetch();
                        }
                    });
                    Effect::new(move |_| {
                        if matches!(delete_action.value().get(), Some(Ok(_))) {
                            archived.refetch();
                        }
                    });

                    let display_name = user.display_name.clone();

                    // ── RT-04 Notification badge (06-06) ──────────────────────────────────
                    // Open a per-user notification WebSocket so the sidebar badge updates live.
                    // spawn_notif_task re-seeds via get_unread_count() on every connect.
                    // on_cleanup drops the WsHandle on navigate-away (Anti-Pattern §717).
                    let unread_count = RwSignal::new(0i64);
                    let badge_pulse = RwSignal::new(false);
                    {
                        let notif_handle = StoredValue::new(
                            Some(spawn_notif_task(unread_count, badge_pulse))
                        );
                        on_cleanup(move || {
                            notif_handle.update_value(|h| { h.take(); });
                        });
                    }

                    // on_new_board is a no-op on the archive page
                    let on_new_board: Callback<()> = Callback::new(|_| {});

                    view! {
                        <div class="lns-app">
                            // Sidebar — shows user's live boards and starred sections (WR-04)
                            <WorkspaceSidebar
                                all_boards=Signal::derive(move || {
                                    boards.get()
                                        .and_then(|r| r.ok())
                                        .unwrap_or_default()
                                })
                                starred_boards=Signal::derive(move || {
                                    starred.get()
                                        .and_then(|r| r.ok())
                                        .unwrap_or_default()
                                })
                                on_star=star_cb
                                unread_count=unread_count
                                badge_pulse=badge_pulse
                            />

                            // Main column
                            <div class="lns-app-main">
                                <WorkspaceTopbar
                                    display_name=display_name
                                    on_new_board=on_new_board
                                />

                                <div class="lns-workspace-content">
                                    <div class="lns-archive-header">
                                        <h1 class="lns-workspace-section-heading">
                                            <Icon name="archive"/>
                                            "Archive"
                                        </h1>
                                        <p class="lns-archive-subtitle">
                                            "Archived boards are hidden from your workspace. Restore them to bring them back."
                                        </p>
                                    </div>

                                    <Suspense fallback=|| ()>
                                        {move || archived.get().map(|result| {
                                            let boards = result.unwrap_or_default();
                                            if boards.is_empty() {
                                                view! {
                                                    <div class="lns-archive-empty">
                                                        <p class="lns-archive-empty-text">"No archived boards."</p>
                                                    </div>
                                                }.into_any()
                                            } else {
                                                view! {
                                                    <div class="lns-archive-grid">
                                                        <For
                                                            each=move || boards.clone()
                                                            key=|b| b.id.clone()
                                                            children=move |board: BoardWithMeta| {
                                                                view! {
                                                                    <ArchivedBoardCard
                                                                        board=board
                                                                        restore_action=restore_action
                                                                        delete_action=delete_action
                                                                    />
                                                                }
                                                            }
                                                        />
                                                    </div>
                                                }.into_any()
                                            }
                                        })}
                                    </Suspense>
                                </div>
                            </div>
                        </div>
                    }.into_any()
                }
            })}
        </Suspense>
    }
}
