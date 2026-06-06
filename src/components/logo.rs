use leptos::prelude::*;

/// LogoMark: three vertical bars aligned to the bottom edge, plus "Lanes" wordmark.
///
/// Logo spec (UI-SPEC):
///   Container height: 18px
///   Bar 1 (left):   width 4px, height 50% (9px),   color var(--text), radius 1.5px
///   Bar 2 (center): width 4px, height 100% (18px),  color var(--text), radius 1.5px
///   Bar 3 (right):  width 4px, height 75% (13.5px), color var(--accent), radius 1.5px
///   Gap between bars: 2px
///   Wordmark: "Lanes", 17px/700, letter-spacing -0.02em, gap from mark 9px
#[component]
pub fn LogoMark() -> impl IntoView {
    view! {
        <div class="lns-logo-mark">
            <div class="lns-logo-bars">
                <span class="lns-logo-bar lns-logo-bar--short"></span>
                <span class="lns-logo-bar lns-logo-bar--full"></span>
                <span class="lns-logo-bar lns-logo-bar--accent"></span>
            </div>
            <span class="lns-logo-wordmark">"Lanes"</span>
        </div>
    }
}
