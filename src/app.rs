use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::{components::Router, components::Routes, components::Route, path};

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/lanes.css"/>
        <Title text="Lanes"/>
        <Router>
            <Routes fallback=|| view! { <p>"Not found."</p> }>
                <Route path=path!("/") view=HomePage/>
            </Routes>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    view! {
        <main class="home">
            <h1>"Lanes"</h1>
            <p>"Your kanban board is loading..."</p>
        </main>
    }
}
