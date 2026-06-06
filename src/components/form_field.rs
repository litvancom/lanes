use leptos::prelude::*;

/// FormField: wraps label + input + optional field error message.
///
/// Spec (UI-SPEC Input spec):
///   padding: 11px 13px, border: 1px var(--border), radius: 6px, font-size: 14px
///   Focus: border-color var(--accent)
///   Error: border-color var(--danger)
///   aria-describedby links error to input
///   Label: 12px/700/-0.005em var(--text-secondary) (UI-SPEC Small)
#[component]
pub fn FormField(
    /// The unique id for this field (used for label-for and error association)
    id: &'static str,
    /// The form field name
    name: &'static str,
    /// Label text
    label: &'static str,
    /// Input type ("text", "email", "password")
    #[prop(default = "text")]
    input_type: &'static str,
    /// autocomplete attribute
    #[prop(default = "")]
    autocomplete: &'static str,
    /// Placeholder text
    #[prop(default = "")]
    placeholder: &'static str,
    /// Optional error message signal — if Some, shows error border + message
    #[prop(optional)]
    error: Option<Signal<Option<String>>>,
    /// Optional suffix element inside the label row (e.g. "Forgot?" link)
    #[prop(optional)]
    label_suffix: Option<AnyView>,
) -> impl IntoView {
    let error_id = format!("{}-error", id);
    // Clone for the two closures that need it
    let error_id_for_attr = error_id.clone();

    let has_error = move || {
        error.map(|e| e.get().is_some()).unwrap_or(false)
    };

    view! {
        <div class="lns-field">
            <div class="lns-field-label-row">
                <label class="lns-label" for=id>{label}</label>
                {label_suffix}
            </div>
            <input
                id=id
                class="lns-input"
                class:lns-input--error=has_error
                type=input_type
                name=name
                autocomplete=autocomplete
                placeholder=placeholder
                aria-describedby=move || if has_error() { error_id_for_attr.clone() } else { String::new() }
            />
            {move || error.and_then(|e| e.get()).map(|msg| view! {
                <span id=error_id.clone() class="lns-field-error">{msg}</span>
            })}
        </div>
    }
}
