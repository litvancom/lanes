use leptos::prelude::*;
use crate::routes::board::BoardSignals;

/// ReconnectToast — shown fixed bottom-center when reconnect_attempts >= 2 (D-01).
///
/// Silent on first transient drop. Appears after the second failed attempt so the user
/// knows something is wrong, without spamming on brief network blips.
/// Dismisses automatically when WS reconnects (reconnect_attempts resets to 0).
///
/// Design: UI-SPEC §6 "Reconnecting…" Toast (§254-279).
#[component]
pub fn ReconnectToast() -> impl IntoView {
    let signals = expect_context::<BoardSignals>();
    let reconnect_attempts = signals.reconnect_attempts;

    view! {
        <Show when=move || { reconnect_attempts.get() >= 2 }>
            <div class="lns-reconnect-toast" role="status" aria-live="polite">
                // Wifi-off SVG icon (16×16, --text-muted stroke)
                <svg
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="var(--text-muted)"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    // wifi-off icon: crossed wifi waves + slash
                    <line x1="1" y1="1" x2="23" y2="23"/>
                    <path d="M16.72 11.06A10.94 10.94 0 0 1 19 12.55"/>
                    <path d="M5 12.55a10.94 10.94 0 0 1 5.17-2.39"/>
                    <path d="M10.71 5.05A16 16 0 0 1 22.56 9"/>
                    <path d="M1.42 9a15.91 15.91 0 0 1 4.7-2.88"/>
                    <path d="M8.53 16.11a6 6 0 0 1 6.95 0"/>
                    <line x1="12" y1="20" x2="12.01" y2="20"/>
                </svg>
                <span>"Reconnecting…"</span>
            </div>
        </Show>
    }
}
