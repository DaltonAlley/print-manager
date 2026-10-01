use std::collections::HashMap;

use super::{WebState, error_page, not_found, validation_page};
use axum::{
    Router,
    extract::{OriginalUri, Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::post,
};

use crate::{
    domain::SettingsInput,
    ui::{settings::SettingsData, shared::ValidationData},
};

pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route("/settings", post(save_settings))
        .route("/settings/tags", post(create_tag))
        .route("/settings/tags/{id}/archive", post(archive_tag))
        .route("/jobs/{job_id}/tags/{tag_id}/assign", post(assign_job_tag))
        .route("/jobs/{job_id}/tags/{tag_id}/remove", post(remove_job_tag))
}

pub(super) async fn create_tag(
    State(state): State<WebState>,
    axum::Form(values): axum::Form<HashMap<String, String>>,
) -> Response {
    let name = values.get("name").map_or("", String::as_str);
    let color = values.get("color_key").map_or("", String::as_str);
    match state.repository.create_tag(name, color).await {
        Ok(Some(_)) => Redirect::to("/settings?saved=1").into_response(),
        Ok(None) => (
            StatusCode::BAD_REQUEST,
            "Use a unique label name and a listed color.",
        )
            .into_response(),
        Err(error) => error_page(&error.to_string()),
    }
}

pub(super) async fn archive_tag(State(state): State<WebState>, Path(id): Path<String>) -> Response {
    match state.repository.archive_tag(&id).await {
        Ok(true) => Redirect::to("/settings?saved=1").into_response(),
        Ok(false) => not_found(),
        Err(error) => error_page(&error.to_string()),
    }
}

pub(super) async fn assign_job_tag(
    State(state): State<WebState>,
    Path((job_id, tag_id)): Path<(String, String)>,
) -> Response {
    match state.repository.job(&job_id).await {
        Ok(Some(job)) if job.completed_at.is_none() => {}
        Ok(Some(_)) => {
            return (StatusCode::CONFLICT, "Completed job labels are read-only.").into_response();
        }
        Ok(None) => return not_found(),
        Err(error) => return error_page(&error.to_string()),
    }
    match state.repository.assign_tag(&job_id, &tag_id).await {
        Ok(true) => Redirect::to(&format!("/jobs/{job_id}?labels=saved")).into_response(),
        Ok(false) => Redirect::to(&format!("/jobs/{job_id}")).into_response(),
        Err(error) => error_page(&error.to_string()),
    }
}

pub(super) async fn remove_job_tag(
    State(state): State<WebState>,
    Path((job_id, tag_id)): Path<(String, String)>,
) -> Response {
    match state.repository.job(&job_id).await {
        Ok(Some(job)) if job.completed_at.is_none() => {}
        Ok(Some(_)) => {
            return (StatusCode::CONFLICT, "Completed job labels are read-only.").into_response();
        }
        Ok(None) => return not_found(),
        Err(error) => return error_page(&error.to_string()),
    }
    match state.repository.unassign_tag(&job_id, &tag_id).await {
        Ok(true) => Redirect::to(&format!("/jobs/{job_id}?labels=saved")).into_response(),
        Ok(false) => not_found(),
        Err(error) => error_page(&error.to_string()),
    }
}

pub(super) async fn save_settings(
    OriginalUri(uri): OriginalUri,
    State(state): State<WebState>,
    axum::Form(input): axum::Form<SettingsInput>,
) -> Response {
    let (tax, turnaround_days) = match input.validate() {
        Ok(values) => values,
        Err(errors) => {
            return validation_page(
                &state,
                uri,
                ValidationData::Settings(SettingsData {
                    business_name: input.business_name,
                    address: input.address,
                    phone: input.phone,
                    email: input.email,
                    has_logo: state
                        .repository
                        .settings()
                        .await
                        .ok()
                        .and_then(|settings| settings.logo_path)
                        .is_some_and(|identifier| {
                            state.storage.managed_logo_path(&identifier).is_ok()
                        }),
                    default_tax_rate: input.default_tax_rate,
                    default_turnaround_days: input.default_turnaround_days,
                    quote_prefix: input.quote_prefix,
                    invoice_prefix: input.invoice_prefix,
                    resend_configured: state.resend_configured(),
                    errors,
                    tags: Vec::new(),
                }),
            )
            .await;
        }
    };
    match state
        .repository
        .save_settings(&input, tax, turnaround_days)
        .await
    {
        Ok(()) => Redirect::to("/settings?saved=1").into_response(),
        Err(e) => error_page(&e.to_string()),
    }
}
