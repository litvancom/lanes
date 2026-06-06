use leptos::prelude::*;

/// CustomCheckbox: 16x16 custom styled checkbox with accessible markup.
///
/// Spec (UI-SPEC Checkbox spec):
///   Control: 16×16px, radius 4px
///   Unchecked: 1.5px border var(--border-strong), bg var(--bg-elevated)
///   Checked: bg var(--text), border var(--text), white 12px SVG checkmark
///   Label: 13px/400 var(--text-secondary), 8px gap
///   Accessibility: role="checkbox", aria-checked, label association
///
/// In v1 the checkbox is cosmetic only — default checked (D-07).
#[component]
pub fn CustomCheckbox(
    /// The form field name (used for the hidden input)
    name: &'static str,
    /// Label text
    label: &'static str,
    /// Whether checked by default (default: true)
    #[prop(default = true)]
    checked: bool,
) -> impl IntoView {
    let checked_signal = RwSignal::new(checked);

    view! {
        <label class="lns-checkbox">
            <span
                class="lns-checkbox-control"
                class:lns-checkbox-control--checked=move || checked_signal.get()
                role="checkbox"
                tabindex="0"
                aria-checked=move || if checked_signal.get() { "true" } else { "false" }
                on:click=move |_| checked_signal.update(|v| *v = !*v)
            >
                // White SVG checkmark (visible when checked)
                {move || if checked_signal.get() {
                    view! {
                        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
                            <path d="M2 6l3 3 5-5" stroke="white" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
                        </svg>
                    }.into_any()
                } else {
                    ().into_any()
                }}
            </span>
            // Hidden input for form submission — sends value when checked
            {move || if checked_signal.get() {
                view! {
                    <input type="hidden" name=name value="on"/>
                }.into_any()
            } else {
                ().into_any()
            }}
            <span class="lns-checkbox-label">{label}</span>
        </label>
    }
}
