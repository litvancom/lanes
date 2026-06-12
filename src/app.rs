use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::{components::Router, components::Routes, components::Route, components::ParentRoute, path};
use crate::routes::workspace::WorkspacePage;
use crate::routes::signup::SignupPage;
use crate::routes::login::LoginPage;
use crate::routes::invite::InviteAcceptPage;
use crate::routes::board::BoardPage;
use crate::routes::archive::ArchivePage;
use crate::routes::card_detail::CardDetailRoute;
use crate::routes::inbox::InboxPage;
use crate::routes::calendar::CalendarPage;
use crate::routes::settings::SettingsPage;

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
                <ParentRoute path=path!("/board/:id") view=BoardPage>
                    <Route path=path!("card/:card_num") view=CardDetailRoute/>
                    <Route path=path!("") view=|| ()/>
                </ParentRoute>
                <Route path=path!("/archive") view=ArchivePage/>
                <Route path=path!("/inbox") view=InboxPage/>
                <Route path=path!("/calendar") view=CalendarPage/>
                <Route path=path!("/settings") view=SettingsPage/>
            </Routes>
        </Router>
    }
}
