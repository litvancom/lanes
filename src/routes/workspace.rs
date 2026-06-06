use leptos::prelude::*;
use leptos::form::ActionForm;
use crate::api::workspace_api::{list_boards, AddBoard};
use crate::components::board_card::BoardCard;
use crate::models::Board;

#[component]
pub fn WorkspacePage() -> impl IntoView {
    // Server-prefetched Resource: calls list_boards during SSR, deserializes on client
    let boards = Resource::new(|| (), |_| async { list_boards().await });

    // ServerAction for the write interaction (D-11)
    let add_action = ServerAction::<AddBoard>::new();

    // Refetch boards after a successful add
    Effect::new(move |_| {
        if add_action.value().get().is_some() {
            boards.refetch();
        }
    });

    view! {
        <div class="workspace-page">
            <h1>"Boards"</h1>

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
    }
}
