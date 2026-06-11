//! Card detail modal shell (Plan 02).
//!
//! Renders design screen 05 over the board surface:
//! - Cover band (120px, gradient from card.cover)
//! - Breadcrumb (in list {name} · {board})
//! - Title (h1, click-to-edit)
//! - Member avatar stack + created-at line
//! - Badges row (Labels / Due date / Priority)
//! - Description (markdown rendered + sanitized, click-to-edit)
//! - Checklist section placeholder
//! - Activity section placeholder
//! - Sidebar (Add to card + Actions groups, #LANES-Cnn footer)
//!
//! Close: backdrop click, × button, or Escape → `use_navigate()` replace to /board/{id}.
//! Width override: 760px via `.lns-card-modal` class (does not touch Phase-3 `.lns-modal-content`).

pub mod activity;
pub mod attachments;
pub mod checklist;
pub mod pickers;
pub mod sidebar;

use leptos::prelude::*;
use leptos_router::components::Redirect;
use crate::models::{ActivityEntry, Attachment, CardDetail, ChecklistItem};
use crate::routes::board::BoardSignals;
use crate::api::card_detail_api::{UpdateCardTitle, UpdateCardDescription};
use crate::components::modal::Modal;
use crate::components::icon::Icon;
use crate::components::card_detail::activity::ActivitySection;
use crate::components::card_detail::attachments::AttachmentsSection;
use crate::components::card_detail::checklist::ChecklistSection;
use crate::components::card_detail::sidebar::SidebarColumn;

/// Validate a CSS color string for safe interpolation into inline styles.
/// Mirrors `safe_cover_color` from kanban_card.rs.
fn safe_cover_color_str(c: &str) -> String {
    let s = c.trim();
    if let Some(inner) = s.strip_prefix("oklch(").and_then(|x| x.strip_suffix(')')) {
        if inner.chars().all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '%' | ' ' | '+' | '-')) {
            return c.to_string();
        }
    }
    if s.starts_with('#') && (s.len() == 7 || s.len() == 4)
        && s[1..].chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return c.to_string();
    }
    // Linear gradients and other values fall back to subtle bg
    "var(--bg-subtle)".to_string()
}

/// Format epoch millis as a relative "N units ago" string.
pub(crate) fn relative_time(created_at_ms: i64) -> String {
    let now_ms: i64;
    #[cfg(feature = "ssr")]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
    }
    #[cfg(all(not(feature = "ssr"), feature = "hydrate"))]
    {
        now_ms = js_sys::Date::now() as i64;
    }
    #[cfg(all(not(feature = "ssr"), not(feature = "hydrate")))]
    {
        now_ms = 0;
    }

    let diff_secs = ((now_ms - created_at_ms) / 1000).max(0);
    let diff_mins = diff_secs / 60;
    let diff_hours = diff_mins / 60;
    let diff_days = diff_hours / 24;
    let diff_weeks = diff_days / 7;
    let diff_months = diff_days / 30;

    if diff_secs < 60 {
        "just now".to_string()
    } else if diff_mins < 60 {
        if diff_mins == 1 { "1 minute ago".to_string() } else { format!("{} minutes ago", diff_mins) }
    } else if diff_hours < 24 {
        if diff_hours == 1 { "1 hour ago".to_string() } else { format!("{} hours ago", diff_hours) }
    } else if diff_days < 7 {
        if diff_days == 1 { "1 day ago".to_string() } else { format!("{} days ago", diff_days) }
    } else if diff_weeks < 5 {
        if diff_weeks == 1 { "1 week ago".to_string() } else { format!("{} weeks ago", diff_weeks) }
    } else if diff_months < 12 {
        if diff_months == 1 { "1 month ago".to_string() } else { format!("{} months ago", diff_months) }
    } else {
        "a long time ago".to_string()
    }
}

/// Card detail modal (design screen 05 shell).
///
/// Mounts over the board surface via the Outlet in BoardPage.
/// Consumes `CardDetail` from the SSR-prefetched Resource in CardDetailRoute.
#[component]
pub fn CardDetailModal(
    detail_data: Resource<Result<CardDetail, ServerFnError>>,
    board_id: String,
    card_num: i64,
) -> impl IntoView {
    let board_id_sv = StoredValue::new(board_id);

    // Show signal drives the Modal shell; flipping to false triggers navigate-back
    let show = RwSignal::new(true);

    // When show flips to false, navigate back to the board (history replace, D-01)
    Effect::new(move |prev: Option<bool>| {
        let current = show.get();
        if let Some(true) = prev {
            if !current {
                let path = format!("/board/{}", board_id_sv.get_value());
                #[cfg(target_arch = "wasm32")]
                {
                    use leptos_router::hooks::use_navigate;
                    use leptos_router::NavigateOptions;
                    let navigate = use_navigate();
                    navigate(&path, NavigateOptions { replace: true, ..Default::default() });
                }
            }
        }
        current
    });

    // Mutation actions for title and description
    let update_title_action = ServerAction::<UpdateCardTitle>::new();
    let update_desc_action = ServerAction::<UpdateCardDescription>::new();

    // Title edit state
    let editing_title = RwSignal::new(false);
    let title_input = RwSignal::new(String::new());

    // Description edit state
    let editing_desc = RwSignal::new(false);
    let desc_input = RwSignal::new(String::new());
    let desc_changed = RwSignal::new(false);

    // Board signals context (for per-card RwSignal<Card> write-through on title save)
    let board_signals: Option<BoardSignals> = use_context::<BoardSignals>();

    let _ = card_num; // used in footer via detail_data

    view! {
        <Suspense fallback=move || view! {
            <div class="lns-modal-loading">"Loading card…"</div>
        }>
            {move || Suspend::new(async move {
                let result = detail_data.await;
                match result {
                    Err(_) => view! { <Redirect path="/"/> }.into_any(),
                    Ok(data) => {
                        let card = StoredValue::new(data.card.clone());
                        let description_html = StoredValue::new(data.description_html.clone());
                        // Modal-scoped reactive watcher signals (mutated by WatchCard action via SidebarColumn)
                        let watcher_count = RwSignal::new(data.watcher_count);
                        let is_watching = RwSignal::new(data.is_watching);
                        let board_members = StoredValue::new(data.board_members.clone());
                        let board_labels = StoredValue::new(data.board_labels.clone());
                        // Breadcrumb context (UI-SPEC §242: "in list {list} · {board}")
                        let list_name_sv = StoredValue::new(data.list_name.clone());
                        let board_name_sv = StoredValue::new(data.board_name.clone());

                        // Modal-scoped checklist_items signal (seeded from CardDetail)
                        let checklist_items: RwSignal<Vec<ChecklistItem>> =
                            RwSignal::new(data.checklist_items.clone());

                        // Modal-scoped activity signal (seeded from CardDetail.activity)
                        let activity: RwSignal<Vec<ActivityEntry>> =
                            RwSignal::new(data.activity.clone());

                        // Modal-scoped attachments signal (seeded from CardDetail.attachments)
                        let attachments: RwSignal<Vec<Attachment>> =
                            RwSignal::new(data.attachments.clone());

                        // Picker visibility signals (one per picker)
                        let show_member_picker = RwSignal::new(false);
                        let show_label_picker = RwSignal::new(false);
                        let show_date_picker = RwSignal::new(false);
                        let show_priority_picker = RwSignal::new(false);

                        let cover_style = card.with_value(|c| {
                            c.cover.as_deref()
                                .map(|cv| format!("background: {}", safe_cover_color_str(cv)))
                                .unwrap_or_else(|| "background: var(--bg-subtle)".to_string())
                        });

                        let cn = card.with_value(|c| c.card_num);
                        let card_id = card.with_value(|c| c.id.clone());
                        let member_ids = card.with_value(|c| c.member_ids.clone());
                        let labels = card.with_value(|c| c.labels.clone());
                        let due_at = card.with_value(|c| c.due_at);
                        let priority = card.with_value(|c| c.priority.clone());
                        let initial_title = card.with_value(|c| c.title.clone());

                        let card_id_sv = StoredValue::new(card_id.clone());
                        let initial_title_sv = StoredValue::new(initial_title.clone());
                        let cover_style_sv = StoredValue::new(cover_style);

                        view! {
                            <Modal show=show>
                                // ── Cover band ──────────────────────────────────────
                                <div
                                    class="lns-modal-cover"
                                    style=move || cover_style_sv.get_value()
                                >
                                    <button
                                        class="lns-modal-close"
                                        aria-label="Close card"
                                        on:click=move |_| show.set(false)
                                    >
                                        <Icon name="close"/>
                                    </button>
                                </div>

                                // ── Body grid ────────────────────────────────────────
                                <div class="lns-modal-body">

                                    // ── Main column ──────────────────────────────────
                                    <div class="lns-modal-main">

                                        // Header: breadcrumb + title + members + created
                                        <div>
                                            <div class="lns-modal-breadcrumb">
                                                <Icon name="list"/>
                                                "in list"
                                                <span style="font-weight: 600">
                                                    {move || list_name_sv.get_value()}
                                                </span>
                                                "·"
                                                {move || board_name_sv.get_value()}
                                            </div>

                                            // Title: view or edit mode
                                            <Show
                                                when=move || editing_title.get()
                                                fallback=move || {
                                                    let cur = initial_title_sv.get_value();
                                                    // Show optimistically updated title from board signals if available
                                                    let display_title = if let Some(bs) = board_signals {
                                                        let cid = card_id_sv.get_value();
                                                        bs.card_signals.with(|cs| {
                                                            cs.get(&cid).map(|sig| sig.get().title.clone())
                                                        }).unwrap_or(cur)
                                                    } else {
                                                        cur
                                                    };
                                                    view! {
                                                        <h1
                                                            id="modal-heading"
                                                            class="lns-modal-title"
                                                            style="cursor: pointer"
                                                            on:click=move |_| {
                                                                title_input.set(initial_title_sv.get_value());
                                                                editing_title.set(true);
                                                            }
                                                        >
                                                            {display_title}
                                                        </h1>
                                                    }
                                                }
                                            >
                                                <input
                                                    class="lns-modal-title-input"
                                                    type="text"
                                                    prop:value=move || title_input.get()
                                                    on:input=move |ev| title_input.set(event_target_value(&ev))
                                                    on:keydown={
                                                        let cid = card_id_sv.get_value();
                                                        let bid = board_id_sv.get_value();
                                                        move |ev: leptos::ev::KeyboardEvent| {
                                                            let saved = initial_title_sv.get_value();
                                                            match ev.key().as_str() {
                                                                "Enter" => {
                                                                    let t = title_input.get_untracked();
                                                                    if t.trim().is_empty() {
                                                                        title_input.set(saved);
                                                                        editing_title.set(false);
                                                                    } else {
                                                                        // Optimistic write-through to per-card signal (D-15)
                                                                        if let Some(bs) = board_signals {
                                                                            let tv = t.trim().to_string();
                                                                            bs.card_signals.with(|cs| {
                                                                                if let Some(sig) = cs.get(&cid) {
                                                                                    sig.update(|c| c.title = tv.clone());
                                                                                }
                                                                            });
                                                                        }
                                                                        update_title_action.dispatch(UpdateCardTitle {
                                                                            board_id: bid.clone(),
                                                                            card_id: cid.clone(),
                                                                            title: t.trim().to_string(),
                                                                        });
                                                                        editing_title.set(false);
                                                                    }
                                                                }
                                                                "Escape" => {
                                                                    title_input.set(saved);
                                                                    editing_title.set(false);
                                                                }
                                                                _ => {}
                                                            }
                                                        }
                                                    }
                                                    on:blur={
                                                        let cid = card_id_sv.get_value();
                                                        let bid = board_id_sv.get_value();
                                                        move |_| {
                                                            let saved = initial_title_sv.get_value();
                                                            let t = title_input.get_untracked();
                                                            if t.trim().is_empty() {
                                                                title_input.set(saved);
                                                                editing_title.set(false);
                                                            } else {
                                                                if let Some(bs) = board_signals {
                                                                    let tv = t.trim().to_string();
                                                                    bs.card_signals.with(|cs| {
                                                                        if let Some(sig) = cs.get(&cid) {
                                                                            sig.update(|c| c.title = tv.clone());
                                                                        }
                                                                    });
                                                                }
                                                                update_title_action.dispatch(UpdateCardTitle {
                                                                    board_id: bid.clone(),
                                                                    card_id: cid.clone(),
                                                                    title: t.trim().to_string(),
                                                                });
                                                                editing_title.set(false);
                                                            }
                                                        }
                                                    }
                                                    autofocus
                                                />
                                            </Show>

                                            // Member avatar stack + created-at
                                            <div style="display: flex; align-items: center; gap: 6px; margin-top: 4px">
                                                <div class="lns-avatar-stack">
                                                    {member_ids.iter().map(|_id| view! {
                                                        <div class="lns-avatar lns-avatar--sm"/>
                                                    }).collect_view()}
                                                </div>
                                                <span style="font-size: 11px; color: var(--text-muted)">
                                                    "· created "
                                                    {relative_time(0)}
                                                </span>
                                            </div>
                                        </div>

                                        // ── Badges row (Labels / Due date / Priority) ────────
                                        <div style="display: flex; gap: 18px; flex-wrap: wrap">
                                            {(!labels.is_empty()).then(|| view! {
                                                <div>
                                                    <div style="font-size: 11px; font-weight: 600; color: var(--text-muted); letter-spacing: 0.04em; text-transform: uppercase; margin-bottom: 4px">
                                                        "Labels"
                                                    </div>
                                                    <div style="display: flex; gap: 4px; flex-wrap: wrap">
                                                        {labels.iter().map(|label| {
                                                            let color = label.color.clone();
                                                            let name = label.name.clone();
                                                            view! {
                                                                <span
                                                                    class="lns-label expanded"
                                                                    style=format!("background: {}", color)
                                                                >
                                                                    {name}
                                                                </span>
                                                            }
                                                        }).collect_view()}
                                                    </div>
                                                </div>
                                            })}
                                            {due_at.map(|due_ms| {
                                                use crate::components::kanban_card::format_due;
                                                let (label, _tone) = format_due(due_ms);
                                                view! {
                                                    <div>
                                                        <div style="font-size: 11px; font-weight: 600; color: var(--text-muted); letter-spacing: 0.04em; text-transform: uppercase; margin-bottom: 4px">
                                                            "Due date"
                                                        </div>
                                                        <span class="lns-tag">
                                                            <Icon name="calendar"/>
                                                            " "
                                                            {label}
                                                        </span>
                                                    </div>
                                                }
                                            })}
                                            {priority.as_deref().map(|p| {
                                                let cls = match p {
                                                    "P1" => "lns-card-prio p1",
                                                    "P2" => "lns-card-prio p2",
                                                    _ => "lns-card-prio",
                                                };
                                                let p_label = match p {
                                                    "P1" => "P1 · High",
                                                    "P2" => "P2 · Medium",
                                                    _ => "P3 · Low",
                                                };
                                                view! {
                                                    <div>
                                                        <div style="font-size: 11px; font-weight: 600; color: var(--text-muted); letter-spacing: 0.04em; text-transform: uppercase; margin-bottom: 4px">
                                                            "Priority"
                                                        </div>
                                                        <span class=cls style="font-size: 11px; padding: 3px 8px">
                                                            {p_label}
                                                        </span>
                                                    </div>
                                                }
                                            })}
                                        </div>

                                        // ── Description section ──────────────────────────────
                                        <div class="lns-modal-section">
                                            <h4>
                                                <Icon name="file"/>
                                                " Description"
                                            </h4>
                                            <Show
                                                when=move || editing_desc.get()
                                                fallback=move || {
                                                    let html = description_html.get_value();
                                                    if html.is_empty() {
                                                        view! {
                                                            <div
                                                                class="lns-modal-desc"
                                                                style="cursor: pointer; color: var(--text-muted)"
                                                                on:click=move |_| {
                                                                    desc_input.set(String::new());
                                                                    desc_changed.set(false);
                                                                    editing_desc.set(true);
                                                                }
                                                            >
                                                                "Add a description…"
                                                            </div>
                                                        }.into_any()
                                                    } else {
                                                        view! {
                                                            // inner_html ONLY on pre-sanitized description HTML (T-05-06)
                                                            <div
                                                                class="lns-modal-desc"
                                                                inner_html=html
                                                                style="cursor: pointer"
                                                                on:click=move |_| {
                                                                    desc_input.set(String::new());
                                                                    desc_changed.set(false);
                                                                    editing_desc.set(true);
                                                                }
                                                            />
                                                        }.into_any()
                                                    }
                                                }
                                            >
                                                <div>
                                                    <textarea
                                                        class="lns-modal-desc-editor"
                                                        placeholder="Add a description…"
                                                        prop:value=move || desc_input.get()
                                                        on:input=move |ev| {
                                                            desc_changed.set(true);
                                                            desc_input.set(event_target_value(&ev));
                                                        }
                                                    />
                                                    <Show when=move || desc_changed.get()>
                                                        <div style="display: flex; gap: 6px; margin-top: 6px">
                                                            <button
                                                                class="lns-btn lns-btn--primary lns-btn--sm"
                                                                on:click=move |_| {
                                                                    update_desc_action.dispatch(UpdateCardDescription {
                                                                        board_id: board_id_sv.get_value(),
                                                                        card_id: card_id_sv.get_value(),
                                                                        description: desc_input.get_untracked(),
                                                                    });
                                                                    // Refetch to get re-rendered sanitized HTML
                                                                    detail_data.refetch();
                                                                    editing_desc.set(false);
                                                                    desc_changed.set(false);
                                                                }
                                                            >
                                                                "Save"
                                                            </button>
                                                            <button
                                                                class="lns-btn lns-btn--ghost lns-btn--sm"
                                                                on:click=move |_| {
                                                                    editing_desc.set(false);
                                                                    desc_changed.set(false);
                                                                }
                                                            >
                                                                "Cancel"
                                                            </button>
                                                        </div>
                                                    </Show>
                                                </div>
                                            </Show>
                                        </div>

                                        // ── Checklist section ──────────────────────────────
                                        <div class="lns-modal-section" id="card-detail-checklist-section">
                                            <ChecklistSection
                                                board_id=board_id_sv.get_value()
                                                card_id=card_id.clone()
                                                checklist_items=checklist_items
                                                card_signal_key=card_id.clone()
                                            />
                                        </div>

                                        // ── Attachments section ───────────────────────────
                                        <AttachmentsSection
                                            board_id=board_id_sv.get_value()
                                            card_id=card_id.clone()
                                            attachments=attachments
                                            card_signal_key=card_id.clone()
                                        />

                                        // ── Activity section ──────────────────────────────
                                        <div class="lns-modal-section">
                                            <ActivitySection
                                                board_id=board_id_sv.get_value()
                                                card_id=card_id.clone()
                                                activity=activity
                                                board_members=board_members.get_value()
                                                card_signal_key=card_id.clone()
                                            />
                                        </div>
                                    </div>

                                    // ── Sidebar (SidebarColumn: Add-to-card + Actions + footer) ──
                                    <SidebarColumn
                                        board_id=board_id_sv.get_value()
                                        card_id=card_id.clone()
                                        card_num=cn
                                        list_id=card.with_value(|c| c.list_id.clone())
                                        board_members=board_members.get_value()
                                        board_labels=board_labels.get_value()
                                        watcher_count=watcher_count
                                        is_watching=is_watching
                                        show_member_picker=show_member_picker
                                        show_label_picker=show_label_picker
                                        show_date_picker=show_date_picker
                                        show_priority_picker=show_priority_picker
                                    />
                                </div>
                            </Modal>
                        }.into_any()
                    }
                }
            })}
        </Suspense>
    }
}
