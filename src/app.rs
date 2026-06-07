use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::{components::Router, components::Routes, components::Route, path};
use crate::routes::workspace::WorkspacePage;
use crate::routes::signup::SignupPage;
use crate::routes::login::LoginPage;
use crate::routes::invite::InviteAcceptPage;
use crate::routes::board::BoardPage;
use crate::routes::archive::ArchivePage;

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/lanes.css"/>
        <Title text="Lanes"/>
        <Link
            rel="preload"
            href="/fonts/Manrope-Variable.woff2"
            as_="font"
            type_="font/woff2"
            crossorigin="anonymous"
        />
        <Link
            rel="preload"
            href="/fonts/JetBrainsMono-Variable.woff2"
            as_="font"
            type_="font/woff2"
            crossorigin="anonymous"
        />
        <Router>
            <Routes fallback=|| view! { <p>"Not found."</p> }>
                <Route path=path!("/") view=WorkspacePage/>
                <Route path=path!("/signup") view=SignupPage/>
                <Route path=path!("/login") view=LoginPage/>
                <Route path=path!("/invite/:token") view=InviteAcceptPage/>
                <Route path=path!("/board/:id") view=BoardPage/>
                <Route path=path!("/archive") view=ArchivePage/>
            </Routes>
        </Router>
    }
}
