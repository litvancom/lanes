use leptos::prelude::*;

/// Inline SVG icon component — 16×16, 1.6 stroke-width, currentColor.
///
/// Renders the glyph set used across the workspace UI. All glyphs use
/// `stroke="currentColor"` / `fill="currentColor"` so callers control
/// color via CSS (e.g. `color: var(--text-muted)`).
///
/// Supported names: grid, star, star-filled, archive, inbox, calendar,
/// plus, chevron, chevron-down, dots, search, filter, bell, users.
/// Unknown names render a 16×16 transparent placeholder.
#[component]
pub fn Icon(
    /// Name of the glyph to render (see list above)
    name: &'static str,
) -> impl IntoView {
    // Build the inner SVG path(s) string based on the glyph name.
    // All icons use viewBox="0 0 16 16", width="16", height="16".
    // Stroke icons: fill="none" stroke="currentColor" stroke-width="1.6"
    //               stroke-linecap="round" stroke-linejoin="round"
    // Fill icons (star-filled): fill="currentColor" stroke="none"

    let inner_html = match name {
        // Four 3×3 squares arranged in a 2×2 grid
        "grid" => r#"<rect x="1" y="1" width="6" height="6" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><rect x="9" y="1" width="6" height="6" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><rect x="1" y="9" width="6" height="6" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><rect x="9" y="9" width="6" height="6" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Five-point star outline
        "star" => r#"<polygon points="8,1.5 9.9,6.3 15,6.8 11.2,10.2 12.4,15 8,12.4 3.6,15 4.8,10.2 1,6.8 6.1,6.3" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Five-point star filled
        "star-filled" => r#"<polygon points="8,1.5 9.9,6.3 15,6.8 11.2,10.2 12.4,15 8,12.4 3.6,15 4.8,10.2 1,6.8 6.1,6.3" fill="currentColor" stroke="none"/>"#,

        // Box with downward arrow (archive into)
        "archive" => r#"<rect x="1.5" y="1.5" width="13" height="3.5" rx="1" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><path d="M2.5 5v8a1 1 0 001 1h9a1 1 0 001-1V5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><path d="M6 9l2 2 2-2" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><path d="M8 7v4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Inbox tray icon
        "inbox" => r#"<path d="M2 10l1.5-6h9L14 10" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><rect x="1.5" y="9.5" width="13" height="5" rx="1" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><path d="M5.5 12h5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Calendar icon
        "calendar" => r#"<rect x="1.5" y="2.5" width="13" height="12" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><path d="M1.5 6.5h13" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><path d="M5 1.5v2" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><path d="M11 1.5v2" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#,

        // Plus / add
        "plus" => r#"<path d="M8 2.5v11M2.5 8h11" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#,

        // Chevron right (default)
        "chevron" => r#"<path d="M6 3l5 5-5 5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Chevron down
        "chevron-down" => r#"<path d="M3 6l5 5 5-5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Three horizontal dots (ellipsis/overflow)
        "dots" => r#"<circle cx="3" cy="8" r="1.2" fill="currentColor"/><circle cx="8" cy="8" r="1.2" fill="currentColor"/><circle cx="13" cy="8" r="1.2" fill="currentColor"/>"#,

        // Magnifying glass
        "search" => r#"<circle cx="7" cy="7" r="4.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><path d="M10.5 10.5l3.5 3.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#,

        // Filter funnel
        "filter" => r#"<path d="M2 3h12l-4.5 5.5v5l-3-1.5V8.5L2 3z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Bell / notification
        "bell" => r#"<path d="M8 1.5a5 5 0 015 5v3.5l1 2H2l1-2V6.5a5 5 0 015-5z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><path d="M6.5 13.5a1.5 1.5 0 003 0" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#,

        // Two people / users
        "users" => r#"<circle cx="6" cy="5.5" r="3" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><path d="M1.5 14.5c0-2.5 2-4 4.5-4s4.5 1.5 4.5 4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><path d="M11 3c1.5 0 2.5 1 2.5 2.5S12.5 8 11 8" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><path d="M13 11c1 .5 2 1.5 2 3.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#,

        // Clock — for due chip
        "clock" => r#"<circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><path d="M8 4.5v3.75l2.5 1.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Chat bubble — for comment count
        "chat" => r#"<path d="M2 3.5a1.5 1.5 0 011.5-1.5h9a1.5 1.5 0 011.5 1.5v6a1.5 1.5 0 01-1.5 1.5H9l-3 2.5V11H3.5A1.5 1.5 0 012 9.5v-6z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Paperclip — for attachment count
        "paperclip" => r#"<path d="M13.5 7.5l-6.5 6.5a4 4 0 01-5.66-5.66l6.72-6.72a2.5 2.5 0 013.54 3.54L5 11.83a1 1 0 01-1.41-1.42L9 5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Check — for done badge and checklist
        "check" => r#"<path d="M2.5 8.5l4 4 7-7" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Tag — for labels toggle
        "tag" => r#"<path d="M2 2h6l6 6-6 6-6-6V2z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><circle cx="6" cy="6" r="1" fill="currentColor"/>"#,

        // X — for composer close
        "x" => r#"<path d="M3 3l10 10M13 3L3 13" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#,

        // Close (same as x, used in modal close button)
        "close" => r#"<path d="M3 3l10 10M13 3L3 13" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#,

        // List — three horizontal lines with bullets
        "list" => r#"<path d="M2.5 4h11M2.5 8h11M2.5 12h11" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>"#,

        // File / document
        "file" => r#"<path d="M3 2h7l3 3v9a1 1 0 01-1 1H3a1 1 0 01-1-1V3a1 1 0 011-1z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><path d="M10 2v4h3" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Flag — for priority
        "flag" => r#"<path d="M3 2v12M3 2h9l-2.5 4L13 10H3" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Eye — for watch action
        "eye" => r#"<path d="M1.5 8s2.5-5 6.5-5 6.5 5 6.5 5-2.5 5-6.5 5-6.5-5-6.5-5z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><circle cx="8" cy="8" r="2" fill="none" stroke="currentColor" stroke-width="1.6"/>"#,

        // Move-to arrow — for move action
        "moveTo" => r#"<path d="M2 8h12M9 3l5 5-5 5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,

        // Unknown glyph — transparent placeholder (same dimensions)
        _ => "",
    };

    view! {
        <svg
            width="16"
            height="16"
            viewBox="0 0 16 16"
            fill="none"
            xmlns="http://www.w3.org/2000/svg"
            aria-hidden="true"
            inner_html=inner_html
        />
    }
}
