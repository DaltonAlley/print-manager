use super::WebState;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use leptos::prelude::provide_context;
use leptos_axum::render_app_async_with_context;

use crate::ui::shared::ValidationData;

pub(super) fn routes() -> Router<WebState> {
    Router::new().route("/style.css", get(style))
}

pub(super) async fn validation_page(state: &WebState, uri: Uri, data: ValidationData) -> Response {
    let context_state = state.clone();
    let options = state.leptos_options.clone();
    let handler = render_app_async_with_context(
        move || {
            provide_context(context_state.clone());
            provide_context(data.clone());
        },
        move || crate::app::shell(options.clone()),
    );
    let request = match Request::builder().uri(uri).body(Body::empty()) {
        Ok(request) => request,
        Err(error) => return error_page(&error.to_string()),
    };
    let mut response = handler(request).await;
    *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
    response
}

pub(super) async fn style() -> impl IntoResponse {
    // Keep primary application and fallback routes on the same embedded workspace styling.
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../style/main.css"),
    )
}

const RAW_APP_RAIL: &str = r#"
<aside class="app-rail" aria-label="Print Manager workspace">
  <a class="rail-brand" href="/" aria-label="Print Manager schedule"><strong>Print Manager</strong></a>
  <nav class="workspace-nav" aria-label="Workspace navigation">
    <a class="workspace-nav-item" href="/"><span class="workspace-nav-mark" aria-hidden="true">S</span><strong>Schedule</strong></a>
    <a class="workspace-nav-item" href="/quotes"><span class="workspace-nav-mark" aria-hidden="true">Q</span><strong>Quotes</strong></a>
    <a class="workspace-nav-item" href="/invoices"><span class="workspace-nav-mark" aria-hidden="true">$</span><strong>Invoices</strong></a>
    <a class="workspace-nav-item" href="/archives"><span class="workspace-nav-mark" aria-hidden="true">A</span><strong>Archives</strong></a>
  </nav>
  <div class="rail-actions">
    <a class="rail-action rail-settings" href="/settings" aria-label="Shop settings"><span aria-hidden="true">⚙</span><span>Settings</span></a>
    <button class="theme-toggle" type="button" role="switch" aria-checked="false" aria-label="Switch to dark mode" data-theme-toggle><span class="theme-toggle-track" aria-hidden="true"><span class="theme-toggle-glyph theme-toggle-sun">☀</span><span class="theme-toggle-glyph theme-toggle-moon">☾</span><span class="theme-toggle-thumb"></span></span><span class="visually-hidden" data-theme-label>Light theme active</span></button>
  </div>
</aside>"#;

pub(super) fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{} · Print Manager</title><link rel="stylesheet" href="/style.css"><script>{}</script></head><body>{}<main>{}</main><script>{}</script><script>{}</script><script>{}</script></body></html>"#,
        esc(title),
        crate::browser_scripts::THEME_INIT,
        RAW_APP_RAIL,
        body,
        crate::browser_scripts::UNSAVED_GUARD,
        crate::browser_scripts::PDF_ACTION,
        crate::browser_scripts::THEME,
    ))
}

pub(super) fn error_page(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        page(
            "Error",
            &format!(
                "<h1>Something went wrong</h1><p role=status>{}. Your saved records are safe.</p>",
                esc(message)
            ),
        ),
    )
        .into_response()
}

pub(super) fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        page(
            "Not found",
            "<h1>Not found</h1><p>That record does not exist.</p>",
        ),
    )
        .into_response()
}

pub(super) fn esc(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
