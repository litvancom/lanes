use leptos::prelude::*;
use crate::models::CardLabel;

/// Validate a label color string for safe CSS interpolation (T-04-06).
/// Accepts oklch(...) and #rrggbb / #rgb shapes only.
/// Returns a safe neutral color on any invalid input.
fn safe_color(color: &str) -> &str {
    let c = color.trim();
    // Accept oklch(...) only if the interior is restricted to a numeric charset.
    // An unconstrained interior allows CSS-declaration injection inside the style
    // attribute (e.g. closing the function and appending @import) even though Leptos
    // escapes the attribute itself (WR-01).
    if let Some(inner) = c.strip_prefix("oklch(").and_then(|x| x.strip_suffix(')')) {
        if inner.chars().all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '%' | ' ' | '+' | '-')) {
            return color;
        }
    }
    // Accept #rrggbb (7 chars) or #rgb (4 chars)
    if c.starts_with('#') && (c.len() == 7 || c.len() == 4)
        && c[1..].chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return color;
    }
    // Fallback neutral
    "#e3e1dc"
}

/// Label chip component (CARD-06).
///
/// When `expanded` is false: renders a narrow 8px colored bar (collapsed state).
/// When `expanded` is true: renders an 18px pill with the label name (expanded state).
///
/// CSS transition on `.lns-label` handles the smooth height change.
/// Color is validated via `safe_color` before interpolation (T-04-06).
#[component]
pub fn LabelChip(
    label: CardLabel,
    expanded: RwSignal<bool>,
) -> impl IntoView {
    let validated_color = safe_color(&label.color).to_string();
    let label_name = label.name.clone();
    let color_for_fallback = validated_color.clone();

    view! {
        <Show
            when=move || expanded.get()
            fallback={
                let c = color_for_fallback.clone();
                move || view! {
                    <span
                        class="lns-label"
                        style=format!("background:{}", c)
                        aria-hidden="true"
                    />
                }
            }
        >
            <span
                class="lns-label expanded"
                style=format!("background:{}", validated_color)
            >
                {label_name.clone()}
            </span>
        </Show>
    }
}
