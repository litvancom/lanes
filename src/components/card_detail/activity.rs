//! Activity section component for the card-detail modal (Plan 04).
//!
//! Renders the comment composer and the interleaved activity feed:
//! - Composer: avatar + textarea (D-09: Save/Cancel revealed only on non-whitespace)
//! - @mention picker: `@`-triggered dropdown of board members → stable user_id resolution (D-10)
//! - Feed: comments + system events, ordered chronologically (D-07)
//!
//! XSS safety: comment bodies are Leptos TEXT NODES (auto-escaped). Never use raw html injection.
//! Only the description uses sanitized html rendering (Pitfall 7, T-05-13).

use leptos::prelude::*;
use crate::models::{ActivityEntry, Card, UserSummary};
use crate::routes::board::BoardSignals;
use crate::api::card_detail_api::AddComment;
use crate::components::icon::Icon;
use super::relative_time;

/// Activity section: comment composer + @mention picker + interleaved feed.
///
/// Props:
/// - `board_id`, `card_id`: passed to the add_comment server action
/// - `activity`: modal-scoped signal seeded from CardDetail.activity
/// - `board_members`: all members on this board (for @mention picker + author display)
/// - `card_signal_key`: card_id string to look up per-card RwSignal<Card> in BoardSignals
#[component]
pub fn ActivitySection(
    board_id: String,
    card_id: String,
    activity: RwSignal<Vec<ActivityEntry>>,
    board_members: Vec<UserSummary>,
    card_signal_key: String,
) -> impl IntoView {
    let board_id = StoredValue::new(board_id);
    let card_id = StoredValue::new(card_id);
    let card_signal_key = StoredValue::new(card_signal_key);
    let board_members = StoredValue::new(board_members);

    let board_signals: Option<BoardSignals> = use_context::<BoardSignals>();

    // Server action for posting a comment
    let add_action = ServerAction::<AddComment>::new();

    // Composer state
    let comment_body = RwSignal::new(String::new());
    let mention_user_ids: RwSignal<Vec<String>> = RwSignal::new(vec![]);

    // D-09: Save/Cancel shown only when textarea has non-whitespace content
    let has_content = move || !comment_body.get().trim().is_empty();

    // @mention picker state
    let show_mention_picker = RwSignal::new(false);
    let mention_filter = RwSignal::new(String::new());

    // On add_action success: optimistically push the returned ActivityEntry + bump comment_count
    {
        let bs = board_signals;
        Effect::new(move |_| {
            if let Some(Ok(entry)) = add_action.value().get() {
                // Push new entry to feed
                activity.update(|v| v.push(entry));
                // Write-through comment_count to per-card RwSignal<Card> (D-15)
                if let Some(bs_ref) = bs {
                    let key = card_signal_key.get_value();
                    bs_ref.card_signals.with(|cs| {
                        if let Some(sig) = cs.get(&key) {
                            sig.update(|c: &mut Card| {
                                c.comment_count += 1;
                            });
                        }
                    });
                }
                // Clear composer
                comment_body.set(String::new());
                mention_user_ids.set(vec![]);
                show_mention_picker.set(false);
            }
        });
    }

    // Derived: filtered board members for the @mention picker
    let filtered_members = move || {
        let filter = mention_filter.get().to_lowercase();
        board_members.with_value(|members| {
            members
                .iter()
                .filter(|m| filter.is_empty() || m.display_name.to_lowercase().contains(&filter))
                .cloned()
                .collect::<Vec<_>>()
        })
    };

    view! {
        <div>
            <h4>
                <Icon name="chat"/>
                " Activity"
            </h4>

            // ── Composer ────────────────────────────────────────────────────
            <div class="lns-activity-composer" style="display: flex; gap: 10px; align-items: flex-start; margin-bottom: 16px">
                // Author avatar placeholder (color resolved per current user)
                <div class="lns-avatar lns-avatar--md" style="flex-shrink: 0"/>

                <div style="flex: 1; position: relative">
                    <textarea
                        class="lns-modal-desc-editor"
                        placeholder="Write a comment…"
                        aria-label="Write a comment"
                        style="min-height: 60px; resize: vertical"
                        prop:value=move || comment_body.get()
                        on:input=move |ev| {
                            let val = event_target_value(&ev);
                            // Detect @mention trigger
                            if let Some(at_pos) = val.rfind('@') {
                                let after_at = &val[at_pos + 1..];
                                // Only open picker if no space after @ (still typing the name)
                                if !after_at.contains(' ') {
                                    mention_filter.set(after_at.to_string());
                                    show_mention_picker.set(true);
                                } else {
                                    show_mention_picker.set(false);
                                }
                            } else {
                                show_mention_picker.set(false);
                            }
                            comment_body.set(val);
                        }
                        on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                            if ev.key() == "Escape" {
                                show_mention_picker.set(false);
                            }
                        }
                    />

                    // @mention picker dropdown
                    <Show when=move || { show_mention_picker.get() }>
                        // Overlay to close on outside click
                        <div
                            style="position: fixed; inset: 0; z-index: 200"
                            on:click=move |_| show_mention_picker.set(false)
                        />
                        <div
                            class="lns-picker-popover"
                            style="position: absolute; z-index: 201; left: 0; top: 100%; min-width: 200px; max-height: 200px; overflow-y: auto; background: var(--bg-elevated); border: 1px solid var(--border); border-radius: var(--radius-sm); box-shadow: var(--shadow-lg); padding: 4px 0"
                        >
                            {move || {
                                let members = filtered_members();
                                if members.is_empty() {
                                    view! {
                                        <div style="padding: 8px 10px; font-size: 13px; color: var(--text-muted)">
                                            "No members found"
                                        </div>
                                    }.into_any()
                                } else {
                                    members.into_iter().map(|member| {
                                        let uid = StoredValue::new(member.id.clone());
                                        let display = StoredValue::new(member.display_name.clone());
                                        let avatar_color = member.avatar_color.clone();
                                        let initials: String = member.display_name
                                            .split_whitespace()
                                            .filter_map(|w| w.chars().next())
                                            .take(2)
                                            .collect();

                                        view! {
                                            <button
                                                style="display: flex; align-items: center; gap: 8px; width: 100%; padding: 6px 10px; background: none; border: none; cursor: pointer; font-size: 13px; text-align: left"
                                                style:color="var(--text)"
                                                on:click=move |ev| {
                                                    ev.stop_propagation();
                                                    let user_id = uid.get_value();
                                                    let name = display.get_value();
                                                    // Replace the @partial-name at end of text with @display_name + space
                                                    let current = comment_body.get_untracked();
                                                    let new_body = if let Some(at_pos) = current.rfind('@') {
                                                        format!("{} @{} ", &current[..at_pos], name)
                                                    } else {
                                                        current
                                                    };
                                                    comment_body.set(new_body);
                                                    // Record the stable user_id (D-10: never parse display names server-side)
                                                    mention_user_ids.update(|ids| {
                                                        if !ids.contains(&user_id) {
                                                            ids.push(user_id.clone());
                                                        }
                                                    });
                                                    show_mention_picker.set(false);
                                                }
                                            >
                                                <div
                                                    class="lns-avatar lns-avatar--sm"
                                                    style=format!("background: {}", avatar_color)
                                                >
                                                    {initials}
                                                </div>
                                                {move || display.get_value()}
                                            </button>
                                        }
                                    }).collect_view().into_any()
                                }
                            }}
                        </div>
                    </Show>

                    // D-09: Save/Cancel row — shown ONLY when textarea has non-whitespace content
                    <Show when=move || { has_content() }>
                        <div style="display: flex; gap: 6px; margin-top: 6px">
                            <button
                                class="lns-btn lns-btn--primary lns-btn--sm"
                                on:click=move |_| {
                                    let body = comment_body.get_untracked();
                                    let ids = mention_user_ids.get_untracked();
                                    if body.trim().is_empty() { return; }
                                    add_action.dispatch(AddComment {
                                        board_id: board_id.get_value(),
                                        card_id: card_id.get_value(),
                                        body,
                                        mention_user_ids: ids,
                                        client_id: use_context::<BoardSignals>()
                                            .and_then(|bs| bs.own_client_id.get_untracked())
                                            .unwrap_or_default(),
                                    });
                                }
                            >
                                "Save"
                            </button>
                            <button
                                class="lns-btn lns-btn--ghost lns-btn--sm"
                                on:click=move |_| {
                                    comment_body.set(String::new());
                                    mention_user_ids.set(vec![]);
                                    show_mention_picker.set(false);
                                }
                            >
                                "Cancel"
                            </button>
                        </div>
                    </Show>
                </div>
            </div>

            // ── Activity feed ────────────────────────────────────────────────
            <div class="lns-activity">
                {move || {
                    activity.get().into_iter().map(|entry| {
                        let entry_type = entry.entry_type.clone();
                        if entry_type == "comment" {
                            render_comment_entry(entry).into_any()
                        } else {
                            render_event_entry(entry).into_any()
                        }
                    }).collect_view()
                }}
            </div>
        </div>
    }
}

/// Render a comment activity entry.
///
/// XSS safety: body text rendered as a TEXT NODE via {entry.text.clone()} — no raw html rendering.
/// Leptos text nodes HTML-escape automatically. (T-05-13 / Pitfall 7)
fn render_comment_entry(entry: ActivityEntry) -> impl IntoView {
    let author_name = entry.author.as_ref().map(|a| a.display_name.clone()).unwrap_or_default();
    let avatar_color = entry.author.as_ref().map(|a| a.avatar_color.clone()).unwrap_or_default();
    let initials: String = author_name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect();
    let body = entry.text.clone(); // plain text — XSS-safe text node below
    let time_str = relative_time(entry.created_at);

    view! {
        <div class="lns-activity-item">
            <div
                class="lns-avatar lns-avatar--md"
                style=format!("background: {}", avatar_color)
            >
                {initials}
            </div>
            <div style="flex: 1">
                <div class="lns-activity-author">
                    {author_name}
                    <span class="lns-activity-time">{time_str}</span>
                </div>
                // TEXT NODE — auto-escaped by Leptos; no raw html rendering (T-05-13)
                <div class="lns-activity-body">{body}</div>
            </div>
        </div>
    }
}

/// Render a system event activity entry.
///
/// Humanizes the event kind + JSON payload into a readable one-line description.
fn render_event_entry(entry: ActivityEntry) -> impl IntoView {
    let author_name = entry.author.as_ref().map(|a| a.display_name.clone()).unwrap_or_else(|| "System".to_string());
    let avatar_color = entry.author.as_ref().map(|a| a.avatar_color.clone()).unwrap_or_default();
    let initials: String = author_name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect();
    let time_str = relative_time(entry.created_at);

    // Humanize the event kind and payload
    let description = humanize_event(&entry.text, entry.payload.as_deref());

    view! {
        <div class="lns-activity-item" style="align-items: center">
            <div
                class="lns-avatar lns-avatar--md"
                style=format!("background: {}", avatar_color)
            >
                {initials}
            </div>
            <div style="flex: 1; font-size: 12px; color: var(--text-secondary)">
                <span style="font-weight: 600; color: var(--text)">{author_name}</span>
                " "
                {description}
                <span class="lns-activity-time">{time_str}</span>
            </div>
        </div>
    }
}

/// Convert event kind + optional JSON payload into a human-readable description.
///
/// Handles: created, moved, archived, member_added, member_removed.
fn humanize_event(kind: &str, payload: Option<&str>) -> String {
    match kind {
        "created" => "created this card".to_string(),
        "archived" => "archived this card".to_string(),
        "moved" => {
            // payload: {"from_list":"...","to_list":"..."}
            if let Some(p) = payload {
                if let Some(to_list) = extract_json_str(p, "to_list") {
                    return format!("moved this card to {}", to_list);
                }
            }
            "moved this card".to_string()
        }
        "member_added" => {
            if let Some(p) = payload {
                if let Some(name) = extract_json_str(p, "user_name") {
                    return format!("added {} to this card", name);
                }
            }
            "added a member".to_string()
        }
        "member_removed" => {
            if let Some(p) = payload {
                if let Some(name) = extract_json_str(p, "user_name") {
                    return format!("removed {} from this card", name);
                }
            }
            "removed a member".to_string()
        }
        other => format!("{} this card", other),
    }
}

/// Extract the value of a top-level string field from a flat JSON object payload.
///
/// Uses `serde_json` (WR-03) so escaped quotes, backslashes, and unicode escapes in
/// user-controlled display names are parsed correctly rather than truncated by a
/// naive substring scan. Returns None if the payload is not valid JSON, the key is
/// absent, or the value is not a string.
fn extract_json_str(json: &str, key: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    value.get(key)?.as_str().map(|s| s.to_string())
}
