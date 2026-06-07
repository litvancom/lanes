use leptos::prelude::*;
use leptos::form::ActionForm;
use leptos_router::components::Redirect;
use crate::api::workspace_api::{list_boards, AddBoard};
use crate::api::auth_api::{get_current_user, Logout};
use crate::components::board_card::BoardCard;
use crate::models::Board;

#[component]
pub fn WorkspacePage() -> impl IntoView {
    // Auth guard: check current user; redirect to /login if unauthenticated (D-12, Open Question 3)
    let current_user = Resource::new(|| (), |_| async { get_current_user().await });

    // Server-prefetched Resource: calls list_boards during SSR, deserializes on client
    let boards = Resource::new(|| (), |_| async { list_boards().await });

    // ServerAction for the write interaction (D-11)
    let add_action = ServerAction::<AddBoard>::new();

    // Logout action (AUTH-03)
    let logout_action = ServerAction::<Logout>::new();

    // Refetch boards after a successful add (not on failed submissions — WR-01)
    Effect::new(move |_| {
        if matches!(add_action.value().get(), Some(Ok(_))) {
            boards.refetch();
        }
    });

    view! {
        <Suspense fallback=|| ()>
            {move || current_user.get().map(|result| match result {
                // Genuinely unauthenticated → redirect to /login (D-12, T-02-09)
                Ok(None) => view! {
                    <Redirect path="/login"/>
                }.into_any(),
                // Transient failure determining auth (session-store hiccup, server-fn error) — do
                // NOT bounce an authenticated user to /login; show a recoverable retry state (WR-05).
                Err(_) => view! {
                    <div class="workspace-page">
                        <p class="board-error">"Something went wrong determining your session."</p>
                        <button
                            type="button"
                            class="lns-btn"
                            on:click=move |_| current_user.refetch()
                        >
                            "Retry"
                        </button>
                    </div>
                }.into_any(),
                Ok(Some(_user)) => view! {
                    <div class="workspace-page">
                        <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:24px;">
                            <h1>"Boards"</h1>
                            // Sign out control (AUTH-03)
                            <ActionForm action=logout_action>
                                <button type="submit" class="lns-btn">"Sign out"</button>
                            </ActionForm>
                        </div>

                        <Suspense fallback=|| view! { <p>"Loading boards..."</p> }>
                            {move || {
                                boards.get().map(|result| match result {
                                    Err(e) => view! {
                                        <p class="board-error">"Error: " {e.to_string()}</p>
                                    }.into_any(),
                                    Ok(bs) => view! {
                                        <div class="board-grid">
                                            <For
                                                each=move || bs.clone()
                                                key=|b| b.id.clone()
                                                children=|board| view! { <BoardCard board=board/> }
                                            />
                                        </div>
                                    }.into_any(),
                                })
                            }}
                        </Suspense>

                        <ActionForm action=add_action>
                            <div class="add-board-form">
                                <input
                                    type="text"
                                    name="name"
                                    placeholder="New board name"
                                />
                                <button type="submit">"Add board"</button>
                            </div>
                        </ActionForm>

                        {move || add_action.value().get().map(|res: Result<Board, ServerFnError>| match res {
                            Err(e) => view! { <p class="board-error">{e.to_string()}</p> }.into_any(),
                            Ok(_) => ().into_any(),
                        })}
                    </div>
                }.into_any(),
            })}
        </Suspense>
    }
}
