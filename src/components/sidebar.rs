use leptos::prelude::*;
use crate::models::BoardWithMeta;
use crate::components::icon::Icon;
use crate::components::logo::LogoMark;
use crate::components::board_card::safe_hex;

/// Workspace sidebar — 248px fixed panel on the left.
///
/// Contains:
/// - Logo row (top)
/// - Nav items: Boards (active on /), Inbox (live badge, D-12 inert route until Phase 7), Calendar (inert), Archive
/// - Starred boards section — hidden entirely if no boards starred (D-04)
/// - All boards list (chip + name links)
/// - Bottom: "Invite teammate" ghost button
///
/// Design spec:
/// - Width: 248px, bg --bg-sidebar
/// - Nav item padding: 7px 10px (handoff-locked, UI-SPEC Spacing Exceptions)
/// - Section headers: 11px/600 uppercase, --text-muted
/// - Inert items: --text-muted, cursor default, aria-disabled (D-12)
///
/// RT-04: Inbox nav item now carries a live `.lns-sidebar-badge` driven by `unread_count`.
/// The route is still inert (Phase 7 wires /inbox); the badge is live now (UI-SPEC §7).
///
/// Threat mitigations:
/// - T-03-22: board names escaped by Leptos view! (no inner_html)
/// - T-03-23: board colors validated by safe_hex() before CSS interpolation
#[component]
pub fn WorkspaceSidebar(
    /// All non-archived boards (for the "BOARDS" section list).
    all_boards: Signal<Vec<BoardWithMeta>>,
    /// User's starred boards (for the "STARRED" section — hidden when empty).
    starred_boards: Signal<Vec<BoardWithMeta>>,
    /// Callback invoked with board_id when the user stars/unstars from the sidebar list.
    on_star: Callback<String>,
    /// Live unread notification count (RT-04). Seeds from server, patched by WS events.
    /// Drives the inbox badge; hidden when 0, shows "99+" above 99 (UI-SPEC §7).
    #[prop(default = RwSignal::new(0))]
    unread_count: RwSignal<i64>,
    /// True for ~200ms when unread_count increments — triggers CSS pulse (UI-SPEC §301).
    #[prop(default = RwSignal::new(false))]
    badge_pulse: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <aside class="lns-sidebar">
            // --- Logo row ---
            <div class="lns-sidebar-logo">
                <LogoMark/>
            </div>

            // --- Nav items ---
            <nav class="lns-sidebar-nav">
                // Boards (active on /)
                <a href="/" class="lns-sidebar-item lns-sidebar-item--active">
                    <Icon name="grid"/>
                    <span>"Boards"</span>
                </a>

                // Inbox — route-inert until Phase 7 (D-12), but carries a live badge (RT-04).
                // The badge shows the live unread count, hides at 0, and shows "99+" above 99.
                <span class="lns-sidebar-item lns-sidebar-item--inert" aria-disabled="true">
                    <Icon name="inbox"/>
                    <span>"Inbox"</span>
                    <Show when=move || { unread_count.get() > 0 }>
                        <span
                            class=move || {
                                if badge_pulse.get() {
                                    "lns-sidebar-badge lns-sidebar-badge--pulse"
                                } else {
                                    "lns-sidebar-badge"
                                }
                            }
                        >
                            {move || {
                                let c = unread_count.get();
                                if c > 99 { "99+".to_string() } else { c.to_string() }
                            }}
                        </span>
                    </Show>
                </span>

                // Calendar — inert (D-12): no route; full markup for pixel fidelity
                <span class="lns-sidebar-item lns-sidebar-item--inert" aria-disabled="true">
                    <Icon name="calendar"/>
                    <span>"Calendar"</span>
                </span>

                // Archive — navigates to /archive
                <a href="/archive" class="lns-sidebar-item">
                    <Icon name="archive"/>
                    <span>"Archive"</span>
                </a>
            </nav>

            // --- Starred section (hidden when empty, D-04) ---
            <Show when=move || !starred_boards.get().is_empty()>
                <div class="lns-sidebar-section">
                    <h3 class="lns-sidebar-section-label">"STARRED"</h3>
                    <For
                        each=move || starred_boards.get()
                        key=|b| b.id.clone()
                        children={
                            let on_star = on_star.clone();
                            move |board| {
                                let c = safe_hex(&board.color);
                                let chip_style = format!(
                                    "background:{c};width:14px;height:14px;border-radius:4px;flex-shrink:0;"
                                );
                                let href = format!("/board/{}", board.id);
                                let board_id_star = board.id.clone();
                                let on_star_inner = on_star.clone();
                                view! {
                                    <div class="lns-sidebar-board-row">
                                        <a href=href class="lns-sidebar-board-link">
                                            <span class="lns-sidebar-chip" style=chip_style/>
                                            <span class="lns-sidebar-board-name">{board.name.clone()}</span>
                                        </a>
                                        <button
                                            class="lns-sidebar-star-btn"
                                            aria-label="Unstar board"
                                            on:click=move |_: leptos::ev::MouseEvent| on_star_inner.run(board_id_star.clone())
                                        >
                                            <Icon name="star-filled"/>
                                        </button>
                                    </div>
                                }
                            }
                        }
                    />
                </div>
            </Show>

            // --- All boards section ---
            <div class="lns-sidebar-section">
                <h3 class="lns-sidebar-section-label">"BOARDS"</h3>
                <For
                    each=move || all_boards.get()
                    key=|b| b.id.clone()
                    children=move |board| {
                        let c = safe_hex(&board.color);
                        let chip_style = format!(
                            "background:{c};width:14px;height:14px;border-radius:4px;flex-shrink:0;"
                        );
                        let href = format!("/board/{}", board.id);
                        view! {
                            <a href=href class="lns-sidebar-board-link lns-sidebar-item">
                                <span class="lns-sidebar-chip" style=chip_style/>
                                <span class="lns-sidebar-board-name">{board.name.clone()}</span>
                            </a>
                        }
                    }
                />
            </div>

            // --- Bottom: invite teammate ---
            <div class="lns-sidebar-footer">
                <button type="button" class="lns-btn lns-btn--ghost lns-sidebar-invite-btn">
                    <Icon name="users"/>
                    "Invite teammate"
                </button>
            </div>
        </aside>
    }
}
