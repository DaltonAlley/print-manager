use std::collections::HashMap;

use super::{WebState, error_page, not_found, validation_page};
use axum::{
    Router,
    extract::{OriginalUri, Path, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Redirect, Response},
    routing::post,
};

use crate::{
    domain::{JobCardUpdateInput, JobCommentInput, JobInput, JobStage},
    ui::{jobs::JobFormData, shared::ValidationData},
};

pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route("/jobs/new", post(create_job))
        .route("/jobs/{id}/edit", post(update_job))
        .route("/jobs/{id}/card", post(update_job_card))
        .route("/jobs/{id}/comments", post(add_job_comment))
        .route("/jobs/{id}/status", post(job_status))
        .route("/jobs/{id}/picked-up", post(job_picked_up))
        .route("/jobs/{id}/invoice", post(job_invoice))
}

pub(super) async fn create_job(
    OriginalUri(uri): OriginalUri,
    State(state): State<WebState>,
    axum::Form(input): axum::Form<JobInput>,
) -> Response {
    save_job_response(&state, uri, None, input).await
}
pub(super) async fn update_job(
    OriginalUri(uri): OriginalUri,
    State(state): State<WebState>,
    Path(id): Path<String>,
    axum::Form(input): axum::Form<JobInput>,
) -> Response {
    save_job_response(&state, uri, Some(&id), input).await
}

pub(super) async fn update_job_card(
    State(state): State<WebState>,
    Path(id): Path<String>,
    axum::Form(input): axum::Form<JobCardUpdateInput>,
) -> Response {
    if let Err(errors) = input.validate() {
        return (StatusCode::UNPROCESSABLE_ENTITY, errors.join("\n")).into_response();
    }
    match state.repository.update_job_card(&id, &input).await {
        Ok(true) => Redirect::to(&format!("/jobs/{id}?card_saved=1")).into_response(),
        Ok(false) => not_found(),
        Err(error) => error_page(&error.to_string()),
    }
}

pub(super) async fn add_job_comment(
    State(state): State<WebState>,
    Path(id): Path<String>,
    axum::Form(input): axum::Form<JobCommentInput>,
) -> Response {
    if let Err(errors) = input.validate() {
        return (StatusCode::UNPROCESSABLE_ENTITY, errors.join("\n")).into_response();
    }
    match state.repository.add_job_comment(&id, &input.comment).await {
        Ok(true) => Redirect::to(&format!("/jobs/{id}?commented=1")).into_response(),
        Ok(false) => not_found(),
        Err(error) => error_page(&error.to_string()),
    }
}
pub(super) async fn save_job_response(
    state: &WebState,
    uri: Uri,
    id: Option<&str>,
    input: JobInput,
) -> Response {
    let invalid_stage_change = match id {
        Some(id) => state
            .repository
            .job(id)
            .await
            .ok()
            .flatten()
            .is_some_and(|job| job.status != input.status),
        None => input.status != "design",
    };
    let validation = input.validate().and_then(|()| {
        if invalid_stage_change {
            Err(vec![
                "Move production stages from the shop workflow board.".into(),
            ])
        } else {
            Ok(())
        }
    });
    if let Err(errors) = validation {
        return validation_page(
            state,
            uri,
            ValidationData::Job(JobFormData {
                id: id.map(str::to_owned),
                title: input.title,
                description: input.description,
                price: input.price,
                customer_name: input.customer_name,
                customer_phone: input.customer_phone,
                customer_email: input.customer_email,
                category: input.category,
                material: input.material,
                status: input.status,
                due_date: input.due_date,
                notes: input.notes,
                quote_id: input.quote_id,
                quote_number: None,
                errors,
            }),
        )
        .await;
    }
    let creating = id.is_none();
    match state.repository.save_job(id, &input).await {
        Ok(job_id) if creating => {
            Redirect::to(&format!("/jobs/{job_id}?created=1")).into_response()
        }
        Ok(job_id) => Redirect::to(&format!("/jobs/{job_id}/edit?saved=1")).into_response(),
        Err(e) => error_page(&e.to_string()),
    }
}
pub(super) async fn job_status(
    State(state): State<WebState>,
    Path(id): Path<String>,
    axum::Form(values): axum::Form<HashMap<String, String>>,
) -> Response {
    let Some(status) = values.get("status") else {
        return (StatusCode::BAD_REQUEST, "Missing status").into_response();
    };
    let Ok(destination_stage) = status.parse::<JobStage>() else {
        return not_found();
    };
    match state
        .repository
        .set_job_status(&id, destination_stage.as_str())
        .await
    {
        Ok(true) => {
            let destination = if values.get("detail").is_some_and(|value| value == "1") {
                format!("/jobs/{id}?moved={}", destination_stage.as_str())
            } else {
                workflow_redirect(&values, WorkflowOutcome::Moved(destination_stage.as_str()))
            };
            Redirect::to(&destination).into_response()
        }
        Ok(false) => not_found(),
        Err(e) => error_page(&e.to_string()),
    }
}

pub(super) async fn job_picked_up(
    State(state): State<WebState>,
    Path(id): Path<String>,
    axum::Form(values): axum::Form<HashMap<String, String>>,
) -> Response {
    match state.repository.set_job_picked_up(&id, true).await {
        Ok(true) => {
            let destination = if values.get("detail").is_some_and(|value| value == "1") {
                format!("/jobs/{id}?picked_up=1")
            } else {
                workflow_redirect(&values, WorkflowOutcome::PickedUp)
            };
            Redirect::to(&destination).into_response()
        }
        Ok(false) => not_found(),
        Err(error) => error_page(&error.to_string()),
    }
}

#[derive(Clone, Copy)]
pub(super) enum WorkflowOutcome<'a> {
    Moved(&'a str),
    PickedUp,
}

pub(super) fn workflow_redirect(
    values: &HashMap<String, String>,
    outcome: WorkflowOutcome<'_>,
) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    for (form_key, query_key) in [
        ("q", "q"),
        ("category", "category"),
        ("filter_status", "status"),
    ] {
        if let Some(value) = values.get(form_key).filter(|value| !value.is_empty()) {
            query.append_pair(query_key, value);
        }
    }
    if values.get("completed").is_some_and(|value| value == "1") {
        query.append_pair("completed", "1");
    }
    match outcome {
        WorkflowOutcome::Moved(stage) => {
            query.append_pair("moved", stage);
        }
        WorkflowOutcome::PickedUp => {
            query.append_pair("picked_up", "1");
        }
    }
    format!("/?{}", query.finish())
}

pub(super) async fn job_invoice(State(state): State<WebState>, Path(id): Path<String>) -> Response {
    match state.repository.create_or_open_job_invoice(&id).await {
        Ok(Some(invoice_id)) => {
            Redirect::to(&format!("/invoices/{invoice_id}/edit?saved=1")).into_response()
        }
        Ok(None) => not_found(),
        Err(error) => error_page(&error.to_string()),
    }
}
