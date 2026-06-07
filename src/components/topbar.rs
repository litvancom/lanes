// Task 2 stub — will be implemented in Task 2.
// This file is declared in mod.rs so the module can be referenced by workspace.rs.
use leptos::prelude::*;

#[component]
pub fn WorkspaceTopbar(
    #[prop(into)] display_name: String,
    on_new_board: Callback<()>,
) -> impl IntoView {
    let _ = display_name;
    let _ = on_new_board;
    view! {
        <div class="lns-topbar">
            // Task 2 implementation pending
        </div>
    }
}
