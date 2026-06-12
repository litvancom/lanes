//! Presence avatar stack for the board header (RT-03 / SC5).
//!
//! Renders up to 4 viewer avatars (28×28px circles, avatar_color bg, initials) and a
//! "+N" overflow chip when more than 4 viewers are present.
//!
//! Current user is excluded by `BoardSignals.viewers` (patched by Task 2 signal logic).
//! No enter/leave animation — the stack simply re-renders on signal change (UI-SPEC §139).

use leptos::prelude::*;
use crate::routes::board::BoardSignals;

/// Board header presence avatar stack.
///
/// Reads `BoardSignals.viewers` from context.
/// Shows up to 4 avatars; overflow as "+N" chip.
/// Renders nothing when no other viewers are present.
#[component]
pub fn PresenceStack() -> impl IntoView {
    let board_signals: Option<BoardSignals> = use_context::<BoardSignals>();

    move || {
        let Some(bs) = board_signals else {
            return None;
        };

        let viewers = bs.viewers.get();
        if viewers.is_empty() {
            return None;
        }

        let max_avatars = 4usize;
        let overflow = viewers.len().saturating_sub(max_avatars);
        let visible: Vec<_> = viewers.into_iter().take(max_avatars).collect();

        Some(view! {
            <div class="lns-presence-stack" role="group" aria-label="Current viewers">
                {visible.into_iter().enumerate().map(|(idx, viewer)| {
                    let first_letter = viewer.display_name
                        .chars()
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .unwrap_or_default();
                    let color = viewer.avatar_color.clone();
                    let title_text = viewer.display_name.clone();
                    // -6px overlap after the first avatar (UI-SPEC §136)
                    let style = if idx == 0 {
                        format!(
                            "background:{};color:var(--text-inverse);width:28px;height:28px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:11px;font-weight:600;border:2px solid var(--bg-elevated);flex-shrink:0;",
                            color
                        )
                    } else {
                        format!(
                            "background:{};color:var(--text-inverse);width:28px;height:28px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:11px;font-weight:600;border:2px solid var(--bg-elevated);flex-shrink:0;margin-left:-6px;",
                            color
                        )
                    };
                    view! {
                        <div
                            class="lns-avatar lns-avatar--md"
                            style=style
                            title=title_text
                            aria-label=viewer.display_name
                        >
                            {first_letter}
                        </div>
                    }
                }).collect_view()}

                {(overflow > 0).then(move || view! {
                    <div class="lns-presence-overflow" title=format!("+{overflow} more")>
                        {format!("+{overflow}")}
                    </div>
                })}
            </div>
        }.into_any())
    }
}
