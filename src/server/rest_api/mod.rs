//! REST API router assembly and OpenAPI document.
//!
//! `api_router()` takes NO arguments and returns `axum::Router<AppState>` with UNBOUND state.
//! The caller (`main.rs`) supplies state via `.with_state(app_state)` on the merged router.
//! It is merged BEFORE `.layer(auth_layer)` so bearer auth is independent of sessions (Pitfall 2).

#[cfg(feature = "ssr")]
pub mod auth;
#[cfg(feature = "ssr")]
pub mod boards;
#[cfg(feature = "ssr")]
pub mod cards;
#[cfg(feature = "ssr")]
pub mod comments;
#[cfg(feature = "ssr")]
pub mod lists;
#[cfg(feature = "ssr")]
pub mod workspaces;

/// Build the `/api/v1` router and `/api/openapi.json` endpoint.
///
/// Returns an unbound `axum::Router<AppState>` — the caller merges it into the app and
/// calls `.with_state(app_state)` on the merged router (plan constraint: no-arg, unbound).
#[cfg(feature = "ssr")]
pub fn api_router() -> axum::Router<crate::server::state::AppState> {
    use axum::{Json, routing::get};
    use utoipa::OpenApi;
    use utoipa_axum::{router::OpenApiRouter, routes};

    use crate::models::rest_dto::{
        BoardDto, CreateBoardReq, UpdateBoardReq,
        WorkspaceDto,
    };
    // NOTE: routes! uses fully-qualified paths, so no fn imports needed here.

    // OpenAPI document — collects schemas from all handler derives.
    #[derive(OpenApi)]
    #[openapi(
        info(
            title = "Lanes API",
            version = "1",
            description = "Lanes self-hosted kanban REST API"
        ),
        components(schemas(
            BoardDto,
            CreateBoardReq,
            UpdateBoardReq,
            WorkspaceDto,
        )),
        security(
            ("bearer_token" = [])
        ),
        modifiers(&SecurityAddon),
        tags(
            (name = "boards", description = "Board CRUD"),
            (name = "workspaces", description = "Workspace descriptor"),
        )
    )]
    struct ApiDoc;

    struct SecurityAddon;
    impl utoipa::Modify for SecurityAddon {
        fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
            if let Some(components) = openapi.components.as_mut() {
                use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
                components.add_security_scheme(
                    "bearer_token",
                    SecurityScheme::Http(
                        HttpBuilder::new()
                            .scheme(HttpAuthScheme::Bearer)
                            .bearer_format("opaque")
                            .build(),
                    ),
                );
            }
        }
    }

    // Build OpenApiRouter — collects handler paths for the OpenAPI spec.
    // routes!() must use fully-qualified paths so the macro can locate the
    // generated __path_<fn> module in the right namespace.
    let (api_routes, openapi) = OpenApiRouter::new()
        // Workspace
        .routes(routes!(crate::server::rest_api::workspaces::get_workspace))
        // Boards collection
        .routes(routes!(
            crate::server::rest_api::boards::list_boards,
            crate::server::rest_api::boards::create_board
        ))
        // Board item
        .routes(routes!(
            crate::server::rest_api::boards::get_board,
            crate::server::rest_api::boards::update_board,
            crate::server::rest_api::boards::delete_board
        ))
        .split_for_parts();

    // Merge the spec-aware routes with the plain openapi.json endpoint
    api_routes
        .route(
            "/api/openapi.json",
            get(move || {
                let spec = openapi.clone();
                async move { Json(spec) }
            }),
        )
}
