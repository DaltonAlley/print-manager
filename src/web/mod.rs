#![allow(clippy::format_collect, clippy::format_push_string)]

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::{DefaultBodyLimit, FromRef},
};
use leptos::config::LeptosOptions;
use leptos::prelude::provide_context;
use leptos_axum::{
    LeptosRoutes, generate_route_list, site_pkg_dir_service, site_pkg_dir_service_route_path,
};

use crate::{
    app_services::AppServices, email::ResendClient, repository::Repository, storage::Storage,
};

mod assets;
mod boundary;
mod documents;
mod jobs;
mod settings;

use boundary::{error_page, not_found, validation_page};

#[cfg(test)]
use boundary::{esc, page};
#[cfg(test)]
use documents::{
    EmailAttemptError, FlightError, begin_flight, email_error_status, email_response,
    parse_document, parse_email, pdf_response, save_document_response,
};
#[cfg(test)]
use jobs::{WorkflowOutcome, save_job_response, workflow_redirect};

#[derive(Clone)]
pub(crate) struct WebState {
    repository: Repository,
    resend: Option<ResendClient>,
    email_in_flight: Arc<Mutex<HashSet<String>>>,
    pdf_in_flight: Arc<Mutex<HashSet<String>>>,
    leptos_options: LeptosOptions,
    storage: Storage,
}

impl WebState {
    #[must_use]
    pub(crate) fn new(
        repository: Repository,
        resend: Option<ResendClient>,
        leptos_options: LeptosOptions,
        storage: Storage,
    ) -> Self {
        Self {
            repository,
            resend,
            email_in_flight: Arc::new(Mutex::new(HashSet::new())),
            pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
            leptos_options,
            storage,
        }
    }

    pub(crate) fn resend_configured(&self) -> bool {
        self.resend.is_some()
    }
}

impl FromRef<WebState> for LeptosOptions {
    fn from_ref(state: &WebState) -> Self {
        state.leptos_options.clone()
    }
}

pub(crate) fn router(state: WebState) -> Router {
    let routes = generate_route_list(crate::app::App);
    let options = state.leptos_options.clone();
    let site_pkg_route = site_pkg_dir_service_route_path(&options);
    let site_pkg_service = site_pkg_dir_service(&options);
    let services = AppServices::new(
        state.repository.clone(),
        state.resend.clone(),
        state.storage.clone(),
    );
    let upload_routes =
        assets::upload_routes().layer(DefaultBodyLimit::max(state.storage.multipart_body_limit()));

    Router::new()
        .route_service(&site_pkg_route, site_pkg_service)
        .merge(boundary::routes())
        .merge(assets::routes())
        .merge(jobs::routes())
        .merge(documents::routes())
        .merge(settings::routes())
        .leptos_routes_with_context(
            &state,
            routes,
            move || provide_context(services.clone()),
            move || crate::app::shell(options.clone()),
        )
        .layer(DefaultBodyLimit::max(256 * 1024))
        .merge(upload_routes)
        .with_state(state)
}

#[cfg(test)]
mod tests;
