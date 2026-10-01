use super::*;
use crate::{
    database::Database,
    domain::{DocumentInput, DocumentKind, JobInput, JobStage, LineInput},
    email::{EmailError, MAX_EMAIL_BODY_BYTES, ResendClient},
    repository::{NewJobAttachment, Repository},
    storage::Storage,
};
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use leptos::config::LeptosOptions;
use std::{collections::HashMap, time::Duration};
use tower::ServiceExt;

fn test_storage() -> Storage {
    Storage::new(
        std::env::temp_dir().join(format!("print-manager-web-test-{}", uuid::Uuid::new_v4())),
        1024 * 1024,
    )
}

fn assert_status_notice(html: &str, message: &str) {
    let message_position = html
        .find(message)
        .unwrap_or_else(|| panic!("missing status message: {message}"));
    let containing_markup = &html[message_position.saturating_sub(300)..message_position];
    assert!(
        containing_markup.contains("role=\"status\""),
        "status message is missing an accessible status role: {message}"
    );
}

#[test]
fn schedule_styles_expose_date_drop_feedback_and_a_fixed_responsive_detail_sheet() {
    let stylesheet = include_str!("../../style/main.css");
    for selector in [
        ".drag-grip {",
        ".sortable-ticket.is-dragging {",
        ".date-column.is-drop-target {",
        ".date-column.is-drop-target::after",
    ] {
        assert!(
            stylesheet.contains(selector),
            "missing drag feedback selector {selector}"
        );
    }
    assert!(stylesheet.contains("content: \"Release to set due date\""));
    assert!(stylesheet.contains("grid-template-columns: repeat(2, minmax(0, 1fr));"));
    assert!(stylesheet.contains("grid-template-columns: 4rem minmax(0, 1fr);"));
    assert!(!stylesheet.contains(".descriptor-chip"));
    assert!(!stylesheet.contains("max-width: calc(15.5rem - 1px)"));
    assert!(stylesheet.contains("max-width: none;\n    align-self: stretch;"));
    let uniform_card_rule = stylesheet
        .split_once(".schedule-ticket.uniform-ticket {")
        .map(|(_, rules)| rules)
        .and_then(|rules| rules.split('}').next())
        .unwrap_or_else(|| panic!("missing uniform job card rule"));
    for declaration in [
        "height: auto",
        "min-height: 9.25rem",
        "max-height: none",
        "align-self: start",
        "grid-template-rows: none",
    ] {
        assert!(
            uniform_card_rule.contains(declaration),
            "uniform job card rule is missing `{declaration}`"
        );
    }
    assert!(stylesheet.contains("grid-template-columns: minmax(0, 4fr) minmax(0, 1fr)"));
    assert!(stylesheet.contains("grid-template-columns: repeat(4, minmax(0, 1fr))"));
    assert!(stylesheet.contains("overflow-x: hidden"));
    assert!(stylesheet.contains(".schedule-columns > .date-column.is-unscheduled"));
    assert!(stylesheet.contains(".filter-disclosure[open] { grid-column: 1 / -1; order: 2; }"));
    assert!(
        stylesheet
            .contains(".filter-disclosure[open] .compact-filters { position: static; width: 100%")
    );

    let sheet_rule = stylesheet
        .split(".detail-dialog {")
        .nth(1)
        .and_then(|rules| rules.split('}').next())
        .unwrap_or_else(|| panic!("missing detail dialog rule"));
    for declaration in [
        "position: fixed",
        "inset: 0 0 0 auto",
        "height: 100dvh",
        "max-height: none",
        "margin: 0",
        "overflow: auto",
        "border-radius: 16px 0 0 16px",
    ] {
        assert!(
            sheet_rule.contains(declaration),
            "detail sheet rule is missing `{declaration}`"
        );
    }
    assert!(
        stylesheet.contains(".detail-dialog .job-detail-layout { grid-template-columns: 1fr; }")
    );
    assert!(stylesheet.contains(
        ".job-detail-layout { display: grid; grid-template-columns: minmax(0, 2fr) minmax(17rem, 1fr);"
    ));
    assert!(stylesheet.contains(
        ".dialog-toolbar::before { margin-right: auto; color: var(--text-strong); content: \"Details\";"
    ));
    assert!(stylesheet.contains(
        ".detail-dialog { width: min(100vw, 38rem); border-left: 0; border-radius: 0; }"
    ));
    assert!(stylesheet.contains("@media (prefers-reduced-motion: reduce)"));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn direct_job_intake_round_trips_price_description_and_arbitrary_contact_text() {
    let database = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repository = Repository::new(database.pool().clone());
    repository
        .ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let app = router(WebState {
        repository: repository.clone(),
        resend: None,
        email_in_flight: Arc::new(Mutex::new(HashSet::new())),
        pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
        leptos_options: LeptosOptions::builder()
            .output_name("print-manager")
            .build(),
        storage: test_storage(),
    });
    let saved = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jobs/new")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("title=Window+graphics&description=Install+the+approved+window+graphics&price=125.50&customer_name=Acme&customer_phone=%2B1+555+0100&customer_email=not-an-email&quote_id="))
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(saved.status(), StatusCode::SEE_OTHER);
    let location = saved.headers()[header::LOCATION]
        .to_str()
        .unwrap_or_else(|error| panic!("{error}"));
    let job_id = location
        .strip_prefix("/jobs/")
        .and_then(|value| value.strip_suffix("?created=1"))
        .unwrap_or_else(|| panic!("unexpected job redirect: {location}"));
    let stored = repository
        .job(job_id)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    assert_eq!(stored.customer_phone, "+1 555 0100");
    assert_eq!(stored.customer_email, "not-an-email");
    assert_eq!(stored.description, "Install the approved window graphics");
    assert_eq!(stored.price_cents, 12_550);
    assert_eq!(stored.status, "design");
    assert!(stored.category.is_empty());
    assert!(stored.material.is_empty());
    assert!(stored.notes.is_empty());
    assert!(stored.due_date.is_some());

    let blank_contact = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jobs/new")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(
                    "title=Counter+sign&description=Counter+sign+with+matte+finish&price=0",
                ))
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(blank_contact.status(), StatusCode::SEE_OTHER);
    let blank_location = blank_contact.headers()[header::LOCATION]
        .to_str()
        .unwrap_or_else(|error| panic!("{error}"));
    let blank_id = blank_location
        .strip_prefix("/jobs/")
        .and_then(|value| value.strip_suffix("?created=1"))
        .unwrap_or_else(|| panic!("unexpected blank-contact redirect: {blank_location}"));
    let blank_job = repository
        .job(blank_id)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    assert!(blank_job.customer_name.is_empty());
    assert!(blank_job.customer_phone.is_empty());
    assert!(blank_job.customer_email.is_empty());

    let detail = app
        .oneshot(
            Request::builder()
                .uri(format!("/jobs/{job_id}"))
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(detail.status(), StatusCode::OK);
    let detail_html = String::from_utf8_lossy(
        &axum::body::to_bytes(detail.into_body(), 1_000_000)
            .await
            .unwrap_or_else(|error| panic!("{error}")),
    )
    .into_owned();
    assert!(detail_html.contains("+1 555 0100"));
    assert!(detail_html.contains("not-an-email"));
    assert!(detail_html.contains("Install the approved window graphics"));
    assert!(detail_html.contains("$125.50"));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn accepted_quote_prefills_job_form_without_creating_until_submit() {
    let database = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repository = Repository::new(database.pool().clone());
    repository
        .ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let quote = DocumentInput {
        status: "accepted".into(),
        customer_name: "Quote customer".into(),
        customer_phone: "+1 555 0199".into(),
        customer_email: "quote@example.test".into(),
        issue_date: "2026-08-24".into(),
        lines: vec![
            LineInput {
                description: "Window graphics".into(),
                quantity: "2".into(),
                unit_label: "panels".into(),
                unit_price: "10.00".into(),
            },
            LineInput {
                description: "Matte laminate".into(),
                quantity: "1".into(),
                unit_label: "job".into(),
                unit_price: "2.50".into(),
            },
        ],
        ..DocumentInput::default()
    };
    let quote_id = repository
        .create_document(
            DocumentKind::Quote,
            &quote
                .validate(DocumentKind::Quote)
                .unwrap_or_else(|errors| panic!("{errors:?}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        repository
            .job_counts()
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .total,
        0
    );
    let app = router(WebState {
        repository: repository.clone(),
        resend: None,
        email_in_flight: Arc::new(Mutex::new(HashSet::new())),
        pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
        leptos_options: LeptosOptions::builder()
            .output_name("print-manager")
            .build(),
        storage: test_storage(),
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jobs/new?quote={quote_id}"))
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(response.status(), StatusCode::OK);
    let html = axum::body::to_bytes(response.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let html = String::from_utf8_lossy(&html);
    assert!(html.contains("<h1>New job</h1>"));
    assert!(html.contains("Quote Q-"));
    assert!(html.contains("Window graphics\nMatte laminate"));
    assert!(html.contains("value=\"22.50\""));
    assert!(html.contains("value=\"Quote customer\""));
    assert!(html.contains("value=\"+1 555 0199\""));
    assert!(html.contains("value=\"quote@example.test\""));
    assert!(html.contains(&format!("name=\"quote_id\" value=\"{quote_id}\"")));
    assert!(html.contains(&format!("href=\"/quotes/{quote_id}\"")));
    for field in ["category", "material", "due_date", "status", "notes"] {
        assert!(
            !html.contains(&format!("name=\"{field}\"")),
            "unexpected new-job field {field}"
        );
    }
    assert_eq!(
        repository
            .job_counts()
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .total,
        0
    );

    let created = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jobs/new")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "title=Approved+window+graphics&description=Install+the+approved+panels&price=22.50&customer_name=Quote+customer&customer_phone=%2B1+555+0199&customer_email=quote%40example.test&quote_id={quote_id}"
                )))
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(created.status(), StatusCode::SEE_OTHER);
    let location = created.headers()[header::LOCATION]
        .to_str()
        .unwrap_or_else(|error| panic!("{error}"));
    let job_id = location
        .strip_prefix("/jobs/")
        .and_then(|value| value.strip_suffix("?created=1"))
        .unwrap_or_else(|| panic!("unexpected job redirect: {location}"));
    let job = repository
        .job(job_id)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    assert_eq!(job.quote_id.as_deref(), Some(quote_id.as_str()));
    assert_eq!(job.description, "Install the approved panels");
    assert_eq!(job.price_cents, 2_250);
    assert_eq!(job.status, "design");
    assert!(job.invoice_id.is_none());
}

#[test]
fn workflow_redirect_encodes_known_local_parameters_and_omits_absent_filters() {
    let values = HashMap::from([
        ("q".into(), "A&B / signage".into()),
        ("category".into(), "Wide & large".into()),
        ("filter_status".into(), "print".into()),
        ("completed".into(), "1".into()),
        ("return_to".into(), "https://example.test/escape".into()),
    ]);
    assert_eq!(
        workflow_redirect(&values, WorkflowOutcome::Moved("finish")),
        "/?q=A%26B+%2F+signage&category=Wide+%26+large&status=print&completed=1&moved=finish"
    );
    assert_eq!(
        workflow_redirect(&HashMap::new(), WorkflowOutcome::PickedUp),
        "/?picked_up=1"
    );
}

async fn resend_endpoint(status: StatusCode, body: &'static str) -> String {
    async fn response(
        State((status, body)): State<(StatusCode, &'static str)>,
    ) -> impl IntoResponse {
        (status, body)
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("{error}"));
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            Router::new()
                .route("/emails", post(response))
                .with_state((status, body)),
        )
        .await;
    });
    format!("http://{address}/emails")
}
#[test]
fn parser_preserves_repeated_lines_and_escapes_html() {
    let parsed=parse_document(b"status=draft&customer_name=A%26B&issue_date=2026-08-20&discount=0&tax_rate=0&line_description=One&line_quantity=1&line_unit=ea&line_price=2.50&line_description=Two&line_quantity=.5&line_unit=hr&line_price=10");
    assert_eq!(parsed.lines.len(), 2);
    assert_eq!(parsed.customer_name, "A&B");
    assert_eq!(esc("<script>"), "&lt;script&gt;");
}

#[test]
fn maximum_multibyte_email_message_fits_raw_cap_and_round_trips() {
    let message = "🖨".repeat(2_000);
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("recipient", "customer@example.test")
        .append_pair("subject", "Quote Q-1")
        .append_pair("message", &message)
        .finish();

    assert!(body.len() > 4 * 1024);
    assert!(body.len() <= MAX_EMAIL_BODY_BYTES);
    let parsed = parse_email(body.as_bytes());
    assert_eq!(parsed.message, message);
    assert!(parsed.validate().is_ok());
    const { assert!(MAX_EMAIL_BODY_BYTES < 256 * 1024) };
}

#[test]
fn raw_error_page_synchronizes_theme_immediately_without_a_hydration_marker() {
    let html = page("Error", "<p>safe</p>").0;
    assert!(html.contains(crate::browser_scripts::UNSAVED_GUARD));
    assert!(html.contains(crate::browser_scripts::PDF_ACTION));
    assert!(html.contains(crate::browser_scripts::THEME));
    assert!(html.contains("theme-toggle-sun"));
    assert!(html.contains("theme-toggle-moon"));
    assert!(!html.contains("data-theme-icon"));
    assert!(!html.contains("<html lang=\"en\" data-print-manager-hydrating"));
    assert!(crate::browser_scripts::THEME.contains("else sync();"));
}

#[test]
fn request_flights_reject_duplicates_and_release_on_drop() {
    let active = Arc::new(Mutex::new(HashSet::new()));
    let flight =
        begin_flight(&active, "quote:one".into()).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        begin_flight(&active, "quote:one".into()).err(),
        Some(FlightError::Duplicate)
    );
    drop(flight);
    assert!(
        active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    );
}

#[test]
fn every_email_attempt_error_has_a_distinct_safe_status() {
    assert_eq!(
        email_error_status(&EmailAttemptError::Repository),
        "repository"
    );
    assert_eq!(email_error_status(&EmailAttemptError::Pdf), "pdf");
    assert_eq!(
        email_error_status(&EmailAttemptError::DeliveryUnknown),
        "unknown"
    );
    for (error, expected) in [
        (EmailError::AttachmentTooLarge, "too-large"),
        (EmailError::DeliveryUnknown, "unknown"),
        (EmailError::ProviderRejected, "rejected"),
        (EmailError::Transport, "network"),
        (EmailError::InvalidConfirmation, "confirmation"),
        (EmailError::InvalidInput("safe".into()), "invalid"),
    ] {
        assert_eq!(
            email_error_status(&EmailAttemptError::Send(error)),
            expected
        );
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn saved_pdf_routes_are_private_and_choose_disposition() {
    let database = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repository = Repository::new(database.pool().clone());
    repository
        .ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let input = DocumentInput {
        customer_name: "Acme".into(),
        issue_date: "2026-08-20".into(),
        lines: vec![LineInput {
            description: "Posters".into(),
            quantity: "1.5".into(),
            unit_label: "each".into(),
            unit_price: "10.00".into(),
        }],
        ..DocumentInput::default()
    };
    let valid = input
        .validate(DocumentKind::Quote)
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    let id = repository
        .create_document(DocumentKind::Quote, &valid)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let state = WebState {
        repository,
        resend: None,
        email_in_flight: Arc::new(Mutex::new(HashSet::new())),
        pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
        leptos_options: LeptosOptions::builder()
            .output_name("print-manager")
            .build(),
        storage: test_storage(),
    };

    let download = pdf_response(&state, &id, DocumentKind::Quote, true).await;
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(
        download.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert!(
        download.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap_or_default()
            .starts_with("attachment;")
    );
    let bytes = axum::body::to_bytes(download.into_body(), 2_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(bytes.starts_with(b"%PDF-"));

    let print = pdf_response(&state, &id, DocumentKind::Quote, false).await;
    assert!(
        print.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap_or_default()
            .starts_with("inline;")
    );
    assert!(
        state
            .pdf_in_flight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    );

    state
        .pdf_in_flight
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(format!("quote:{id}"));
    let duplicate = pdf_response(&state, &id, DocumentKind::Quote, true).await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    assert_eq!(
        duplicate.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    let duplicate = axum::body::to_bytes(duplicate.into_body(), 10_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(String::from_utf8_lossy(&duplicate).contains("already being prepared"));
    state
        .pdf_in_flight
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();

    let missing = pdf_response(&state, "missing", DocumentKind::Quote, true).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert!(
        state
            .pdf_in_flight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    );

    let app = router(state);
    let detail = app
        .oneshot(
            Request::builder()
                .uri(format!("/quotes/{id}"))
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let detail = axum::body::to_bytes(detail.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let detail = String::from_utf8_lossy(&detail);
    assert!(detail.contains("data-pdf-action=\"download\""));
    assert!(detail.contains("data-pdf-action=\"print\""));
    assert!(detail.contains("href=\"/quotes/"));
    assert!(detail.contains("target=\"_blank\""));
    assert!(detail.contains("rel=\"noopener\""));
    assert!(detail.contains("id=\"quote-pdf-status\""));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn ordinary_saves_reject_terminal_transitions_and_confirmed_actions_work() {
    let database = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repository = Repository::new(database.pool().clone());
    repository
        .ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let document_input = DocumentInput {
        customer_name: "Terminal Guard Customer".into(),
        issue_date: "2026-08-20".into(),
        lines: vec![LineInput {
            description: "Proof".into(),
            quantity: "1".into(),
            unit_label: "each".into(),
            unit_price: "25.00".into(),
        }],
        ..DocumentInput::default()
    };
    let valid = document_input
        .validate(DocumentKind::Quote)
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    let document_id = repository
        .create_document(DocumentKind::Quote, &valid)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let job_input = JobInput {
        title: "Guarded Job".into(),
        customer_name: "Terminal Guard Customer".into(),
        category: "Proofing".into(),
        status: "design".into(),
        ..JobInput::default()
    };
    let job_id = repository
        .save_job(None, &job_input)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let state = WebState {
        repository: repository.clone(),
        resend: None,
        email_in_flight: Arc::new(Mutex::new(HashSet::new())),
        pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
        leptos_options: LeptosOptions::builder()
            .output_name("print-manager")
            .build(),
        storage: test_storage(),
    };
    let _app = router(state.clone());

    let mut declined = document_input;
    declined.status = "declined".into();
    let response = save_document_response(
        &state,
        format!("/quotes/{document_id}/edit")
            .parse()
            .unwrap_or_else(|error| panic!("{error}")),
        DocumentKind::Quote,
        Some(&document_id),
        declined,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let mut done = job_input;
    done.status = "complete".into();
    let response = save_job_response(
        &state,
        format!("/jobs/{job_id}/edit")
            .parse()
            .unwrap_or_else(|error| panic!("{error}")),
        Some(&job_id),
        done,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        repository
            .document(&document_id, DocumentKind::Quote)
            .await
            .unwrap_or(None)
            .map(|document| document.document.status),
        Some("draft".into())
    );
    assert_eq!(
        repository
            .job(&job_id)
            .await
            .unwrap_or(None)
            .map(|job| job.status),
        Some("design".into())
    );

    assert!(
        repository
            .decline_quote(&document_id)
            .await
            .unwrap_or(false)
    );
    assert!(
        repository
            .set_job_status(&job_id, "complete")
            .await
            .unwrap_or(false)
    );
    assert_eq!(
        repository
            .document(&document_id, DocumentKind::Quote)
            .await
            .unwrap_or(None)
            .map(|document| document.document.status),
        Some("declined".into())
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn email_metadata_changes_only_after_success_and_duplicate_is_rejected() {
    let database = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repository = Repository::new(database.pool().clone());
    repository
        .ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let valid = DocumentInput {
        customer_name: "Unchanged Customer".into(),
        issue_date: "2026-08-20".into(),
        lines: vec![LineInput {
            description: "Posters".into(),
            quantity: "1".into(),
            unit_label: "each".into(),
            unit_price: "10.00".into(),
        }],
        ..DocumentInput::default()
    }
    .validate(DocumentKind::Quote)
    .unwrap_or_else(|errors| panic!("{errors:?}"));
    let id = repository
        .create_document(DocumentKind::Quote, &valid)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let failed_endpoint = resend_endpoint(StatusCode::BAD_REQUEST, "{}").await;
    let mut state = WebState {
        repository: repository.clone(),
        resend: Some(
            ResendClient::with_endpoint(
                "secret".into(),
                "shop@example.test".into(),
                &failed_endpoint,
                Duration::from_secs(1),
            )
            .unwrap_or_else(|error| panic!("{error}")),
        ),
        email_in_flight: Arc::new(Mutex::new(HashSet::new())),
        pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
        leptos_options: LeptosOptions::builder()
            .output_name("print-manager")
            .build(),
        storage: test_storage(),
    };
    let body = b"recipient=customer%40example.test&subject=Quote&message=Attached";

    let failed = email_response(
        &state,
        format!("/quotes/{id}/edit")
            .parse()
            .unwrap_or_else(|e| panic!("{e}")),
        &id,
        DocumentKind::Quote,
        body,
    )
    .await;
    assert_eq!(failed.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        failed.headers()[header::LOCATION],
        format!("/quotes/{id}?email=rejected")
    );
    let unchanged = repository
        .document(&id, DocumentKind::Quote)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert_eq!(unchanged.document.customer_name, "Unchanged Customer");
    let email_record: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT last_emailed_at,resend_message_id FROM documents WHERE id=?")
            .bind(&id)
            .fetch_one(database.pool())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(email_record, (None, None));

    state
        .email_in_flight
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(format!("quote:{id}"));
    let duplicate = email_response(
        &state,
        format!("/quotes/{id}/edit")
            .parse()
            .unwrap_or_else(|e| panic!("{e}")),
        &id,
        DocumentKind::Quote,
        body,
    )
    .await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    state
        .email_in_flight
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();

    let success_endpoint = resend_endpoint(StatusCode::OK, r#"{"id":"provider-456"}"#).await;
    state.resend = Some(
        ResendClient::with_endpoint(
            "secret".into(),
            "shop@example.test".into(),
            &success_endpoint,
            Duration::from_secs(1),
        )
        .unwrap_or_else(|error| panic!("{error}")),
    );
    let sent = email_response(
        &state,
        format!("/quotes/{id}/edit")
            .parse()
            .unwrap_or_else(|e| panic!("{e}")),
        &id,
        DocumentKind::Quote,
        body,
    )
    .await;
    assert_eq!(sent.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        sent.headers()[header::LOCATION],
        format!("/quotes/{id}?email=sent")
    );
    let email_record: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT last_emailed_at,resend_message_id FROM documents WHERE id=?")
            .bind(&id)
            .fetch_one(database.pool())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    assert!(email_record.0.is_some());
    assert_eq!(email_record.1.as_deref(), Some("provider-456"));

    let invalid = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/quotes/{id}/email"))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(
                    "recipient=missing-domain&subject=Keep+this+subject&message=Keep+this+message",
                ))
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = axum::body::to_bytes(invalid.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("missing-domain"));
    assert!(html.contains("Keep this subject"));
    assert!(html.contains("Keep this message"));
    assert!(html.contains("Enter one valid recipient email address"));
    assert!(html.contains("Unchanged Customer"));

    let excessive = router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/quotes/{id}/email"))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(vec![b'x'; MAX_EMAIL_BODY_BYTES + 1]))
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(excessive.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = axum::body::to_bytes(excessive.into_body(), 1_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(String::from_utf8_lossy(&body).contains("saved document is safe"));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn live_workflow_route_is_leptos_ssr_and_escapes_record_values() {
    let database = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repository = Repository::new(database.pool().clone());
    repository
        .ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let active = repository
        .save_job(
            None,
            &JobInput {
                title: "<script>alert('unsafe')</script>".into(),
                customer_name: "Full customer identity & partners".into(),
                category: "Business cards".into(),
                status: "design".into(),
                due_date: "2026-08-30".into(),
                ..JobInput::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    for (name, color) in [("Alpha", "blue"), ("Beta", "green"), ("Gamma", "amber")] {
        let tag = repository
            .create_tag(name, color)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!());
        assert!(
            repository
                .assign_tag(&active, &tag.id)
                .await
                .unwrap_or(false)
        );
    }
    let attachment_hash = "a".repeat(64);
    let _attachment = repository
        .create_attachment(
            &active,
            &NewJobAttachment {
                original_name: "proof <safe>.pdf",
                stored_name: "board-proof.pdf",
                media_type: "application/pdf",
                byte_size: 2_048,
                sha256: &attachment_hash,
            },
        )
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    let pickup = repository
        .save_job(
            None,
            &JobInput {
                title: "Pickup ready".into(),
                customer_name: "Ready customer".into(),
                category: "Posters".into(),
                status: "complete".into(),
                due_date: "2020-01-01".into(),
                ..JobInput::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let unpaid = repository
        .save_job(
            None,
            &JobInput {
                title: "Collected unpaid".into(),
                customer_name: "Account customer".into(),
                category: "Signs".into(),
                status: "complete".into(),
                ..JobInput::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(repository.complete_job(&pickup).await.unwrap_or(false));
    assert!(repository.complete_job(&unpaid).await.unwrap_or(false));
    repository
        .create_or_open_job_invoice(&unpaid)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        repository
            .set_job_picked_up(&unpaid, true)
            .await
            .unwrap_or(false)
    );
    let app = router(WebState {
        repository: repository.clone(),
        resend: None,
        email_in_flight: Arc::new(Mutex::new(HashSet::new())),
        pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
        leptos_options: LeptosOptions::builder()
            .output_name("print-manager")
            .build(),
        storage: test_storage(),
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(response.status(), StatusCode::OK);
    let html = axum::body::to_bytes(response.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let html = String::from_utf8(html.to_vec()).unwrap_or_else(|error| panic!("{error}"));
    assert!(html.contains("<html lang=\"en\" data-print-manager-hydrating"));
    assert!(html.contains("data-leptos-view=\"jobs\""));
    assert!(html.contains("aria-label=\"Print jobs sorted by due date\""));
    assert!(html.contains("class=\"schedule-heading\""));
    assert!(html.contains("Your shop at a glance"));
    assert!(html.contains("class=\"schedule-health"));
    assert!(!html.contains("Due-date schedule"));
    assert!(html.contains("class=\"app-rail\""));
    assert!(html.contains("theme-toggle-sun"));
    assert!(html.contains("theme-toggle-moon"));
    assert!(html.contains("aria-label=\"Workspace navigation\""));
    assert!(html.contains("aria-current=\"page\""));
    assert!(!html.contains("timeline-row timeline-lane"));
    assert!(!html.contains("Workflow status"));
    assert!(!html.contains("id=\"quote-drawer\""));
    assert!(!html.contains("quote-drawer-trigger"));
    assert!(html.contains("href=\"/quotes\""));
    assert!(!html.contains("lane-archives"));
    assert!(html.contains("class=\"button board-new-job\""));
    assert!(html.contains("href=\"/jobs/new\""));
    assert!(!html.contains("quick-job-create"));
    assert!(!html.contains("quick=1"));
    assert!(html.contains("Today &amp; overdue"));
    assert!(!html.contains("data-date-column=\"overdue\""));
    assert!(html.contains("data-date-column=\"later-or-undated\""));
    assert!(html.contains("class=\"schedule-columns\""));
    assert!(html.contains("class=\"job-ticket schedule-ticket uniform-ticket sortable-ticket"));
    assert!(html.contains("state-design"));
    assert!(html.contains("class=\"ticket-customer\""));
    assert!(html.contains("Full customer identity &amp; partners"));
    assert!(html.contains(&format!("href=\"/jobs/{active}\"")));
    assert!(html.contains(&format!("data-job-id=\"{active}\"")));
    assert!(html.contains("Business cards"));
    assert!(html.contains("Later / Undated"));
    assert!(!html.contains("invoice-badge"));
    let active_marker = html
        .find(&format!("data-job-id=\"{active}\""))
        .unwrap_or_else(|| panic!());
    let card_start = html[..active_marker]
        .rfind("<article")
        .unwrap_or_else(|| panic!());
    let card_end = active_marker
        + html[active_marker..]
            .find("</article>")
            .unwrap_or_else(|| panic!());
    let active_card = &html[card_start..card_end];
    assert!(!active_card.contains("Alpha"));
    assert!(!active_card.contains("Beta"));
    assert!(!active_card.contains("Gamma"));
    assert!(active_card.contains("descriptor-list"));
    assert!(active_card.contains("descriptor-item descriptor-product"));
    assert!(active_card.contains("aria-label=\"Job details\""));
    assert!(active_card.contains("Product"));
    assert!(!active_card.contains("descriptor-material"));
    assert!(!active_card.contains("Billing"));
    assert!(!active_card.contains("No invoice"));
    assert!(!active_card.contains("Due Aug 30"));
    assert!(active_card.contains("class=\"ticket-due overdue-tag\""));
    assert!(active_card.contains(">Overdue</span>"));
    assert!(active_card.contains("class=\"status status-design\""));
    assert!(!active_card.contains("data-file-carousel"));
    assert!(!active_card.contains("proof &lt;safe&gt;.pdf"));
    assert!(active_card.contains(&format!("data-card-detail=\"/jobs/{active}\"")));
    assert!(active_card.contains(&format!(
        "<h3><a href=\"/jobs/{active}\" rel=\"external\" data-detail-link"
    )));
    let grip_class = active_card
        .find("class=\"drag-grip\"")
        .unwrap_or_else(|| panic!());
    let grip_start = active_card[..grip_class]
        .rfind("<span")
        .unwrap_or_else(|| panic!());
    let grip_end = grip_start
        + active_card[grip_start..]
            .find('>')
            .unwrap_or_else(|| panic!());
    let grip = &active_card[grip_start..grip_end];
    assert!(grip.contains("aria-hidden=\"true\""));
    assert!(!grip.contains("tabindex"));
    assert!(!grip.contains("draggable"));
    assert!(!grip.contains("aria-describedby"));
    assert!(!grip.contains("aria-grabbed"));
    assert!(html.contains("View job"));
    assert!(!html.contains("ticket-details"));
    assert!(html.contains("Awaiting pickup"));
    let pickup_marker = html
        .find(&format!("data-job-id=\"{pickup}\""))
        .unwrap_or_else(|| panic!());
    let pickup_start = html[..pickup_marker]
        .rfind("<article")
        .unwrap_or_else(|| panic!());
    let pickup_end = pickup_marker
        + html[pickup_marker..]
            .find("</article>")
            .unwrap_or_else(|| panic!());
    let pickup_card = &html[pickup_start..pickup_end];
    assert!(pickup_card.contains("Due Jan 1, 2020"));
    assert!(!pickup_card.contains("Overdue"));
    assert!(html.contains("Picked up"));
    let unpaid_marker = html
        .find(&format!("data-job-id=\"{unpaid}\""))
        .unwrap_or_else(|| panic!());
    let unpaid_start = html[..unpaid_marker]
        .rfind("<article")
        .unwrap_or_else(|| panic!());
    let unpaid_end = unpaid_marker
        + html[unpaid_marker..]
            .find("</article>")
            .unwrap_or_else(|| panic!());
    let unpaid_card = &html[unpaid_start..unpaid_end];
    assert!(unpaid_card.contains("Picked up"));
    assert!(!unpaid_card.contains("stage"));
    assert!(!unpaid_card.contains("draggable=\"true\""));
    assert!(!unpaid_card.contains("tabindex="));
    assert!(unpaid_card.contains(&format!(
        "<h3><a href=\"/jobs/{unpaid}\" rel=\"external\" data-detail-link"
    )));
    assert!(!unpaid_card.contains("action=\"/board/move\""));
    assert!(!html.contains("Confirm pickup"));
    assert!(!html.contains("Choose stage…"));
    assert!(html.contains("draggable=\"true\""));
    assert!(html.contains("tabindex=\"0\""));
    assert!(html.contains("aria-describedby=\"schedule-drag-instructions\""));
    assert!(html.contains("aria-grabbed=\"false\""));
    assert!(html.contains("aria-live=\"polite\""));
    assert!(html.contains("Alt+Left Arrow"));
    assert!(html.contains(&format!("id=\"schedule-card-{active}\"")));
    assert!(!html.contains("sort-controls"));
    assert!(!html.contains("action=\"/board/move\""));
    assert!(!html.contains("name=\"target_lane\""));
    assert!(!html.contains("data-job-board"));
    assert!(!html.contains("Closed archive results"));
    assert!(html.contains("/pkg/print-manager.js"));
    assert!(html.contains("&lt;script&gt;alert('unsafe')&lt;/script&gt;"));
    assert!(!html.contains("<script>alert('unsafe')</script>"));
    assert!(
        html.find("board-search").unwrap_or(usize::MAX)
            < html.find("class=\"board-new-job\"").unwrap_or(usize::MAX)
    );

    let moved = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/jobs/{active}/status"))
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "q=Full+customer&category=Business+cards&filter_status=waiting&completed=1&status=waiting",
                    ))
                    .unwrap_or_else(|error| panic!("{error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(moved.status(), StatusCode::SEE_OTHER);
    let moved_location = moved.headers()[header::LOCATION]
        .to_str()
        .unwrap_or_else(|error| panic!("{error}"))
        .to_owned();
    assert_eq!(
        moved_location,
        "/?q=Full+customer&category=Business+cards&status=waiting&completed=1&moved=waiting"
    );
    assert_eq!(
        repository
            .job(&active)
            .await
            .unwrap_or(None)
            .map(|job| job.status),
        Some("waiting".into())
    );

    let moved_page = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&moved_location)
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let moved_html = axum::body::to_bytes(moved_page.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let moved_html = String::from_utf8_lossy(&moved_html);
    assert_status_notice(&moved_html, "Job updated.");
    assert!(moved_html.contains("name=\"q\" value=\"Full customer\""));
    assert!(moved_html.contains("name=\"category\" value=\"Business cards\""));
    assert!(moved_html.contains("name=\"status\""));
    for state in [
        "Designing",
        "Waiting for customer",
        "Ready to print",
        "Ready to finish",
        "Completed",
    ] {
        assert!(moved_html.contains(state), "missing direct state {state}");
    }
    assert!(!moved_html.contains("name=\"completed\""));

    let picked_up = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/jobs/{pickup}/picked-up"))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(
                    "q=Pickup+ready&category=Posters&filter_status=complete&completed=1",
                ))
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(picked_up.status(), StatusCode::SEE_OTHER);
    let picked_up_location = picked_up.headers()[header::LOCATION]
        .to_str()
        .unwrap_or_else(|error| panic!("{error}"))
        .to_owned();
    assert_eq!(
        picked_up_location,
        "/?q=Pickup+ready&category=Posters&status=complete&completed=1&picked_up=1"
    );
    let picked_up_page = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&picked_up_location)
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let picked_up_html = axum::body::to_bytes(picked_up_page.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_status_notice(
        &String::from_utf8_lossy(&picked_up_html),
        "Pickup confirmed.",
    );

    let missing_pickup = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jobs/missing/picked-up")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(missing_pickup.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn stage_route_cannot_move_picked_up_or_closed_jobs() {
    let database = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repository = Repository::new(database.pool().clone());
    repository
        .ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let mut job_ids = Vec::new();
    for paid in [false, true] {
        let job_id = repository
            .save_job(
                None,
                &JobInput {
                    title: format!("Picked up {paid}"),
                    customer_name: "Acme".into(),
                    category: "Posters".into(),
                    ..JobInput::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let invoice_id = repository
            .create_or_open_job_invoice(&job_id)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!());
        assert!(
            repository
                .set_job_status(&job_id, "complete")
                .await
                .unwrap_or(false)
        );
        assert!(
            repository
                .set_job_picked_up(&job_id, true)
                .await
                .unwrap_or(false)
        );
        if paid {
            assert!(repository.mark_paid(&invoice_id).await.unwrap_or(false));
        }
        job_ids.push(job_id);
    }
    let app = router(WebState {
        repository: repository.clone(),
        resend: None,
        email_in_flight: Arc::new(Mutex::new(HashSet::new())),
        pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
        leptos_options: LeptosOptions::builder()
            .output_name("print-manager")
            .build(),
        storage: test_storage(),
    });

    for job_id in job_ids {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/jobs/{job_id}/status"))
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("status=finish"))
                    .unwrap_or_else(|error| panic!("{error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            repository
                .job(&job_id)
                .await
                .unwrap_or(None)
                .unwrap_or_else(|| panic!())
                .status,
            "complete"
        );
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn job_empty_states_distinguish_population_completion_and_filter_misses() {
    let database = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repository = Repository::new(database.pool().clone());
    repository
        .ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let state = |repository| WebState {
        repository,
        resend: None,
        email_in_flight: Arc::new(Mutex::new(HashSet::new())),
        pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
        leptos_options: LeptosOptions::builder()
            .output_name("print-manager")
            .build(),
        storage: test_storage(),
    };
    let empty_app = router(state(repository.clone()));
    let empty = empty_app
        .oneshot(
            Request::builder()
                .uri("/?completed=1")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let empty = axum::body::to_bytes(empty.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let empty = String::from_utf8_lossy(&empty);
    assert!(empty.contains("class=\"schedule-empty-state\""));
    assert!(empty.contains("The schedule is clear"));
    assert!(!empty.contains("class=\"empty\""));
    assert!(!empty.contains("class=\"timeline-row timeline-lane"));

    let id = repository
        .save_job(
            None,
            &JobInput {
                title: "Finished poster".into(),
                customer_name: "Acme".into(),
                category: "Posters".into(),
                status: "complete".into(),
                due_date: "2020-08-19".into(),
                ..JobInput::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        repository
            .set_job_status(&id, "complete")
            .await
            .unwrap_or(false)
    );
    let invoice = repository
        .create_or_open_job_invoice(&id)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert!(
        repository
            .set_job_picked_up(&id, true)
            .await
            .unwrap_or(false)
    );
    assert!(repository.mark_paid(&invoice).await.unwrap_or(false));
    sqlx::query("UPDATE jobs SET completed_at='2020-08-20T00:00:00Z' WHERE id=?")
        .bind(&id)
        .execute(database.pool())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    for second in 0..10 {
        let recent = repository
            .save_job(
                None,
                &JobInput {
                    title: format!("Recent {second}"),
                    customer_name: "Recent customer".into(),
                    category: "Posters".into(),
                    ..JobInput::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(repository.complete_job(&recent).await.unwrap_or(false));
        sqlx::query("UPDATE jobs SET completed_at=? WHERE id=?")
            .bind(format!("2026-08-20T00:00:{second:02}Z"))
            .bind(recent)
            .execute(database.pool())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }
    let app = router(state(repository));
    let completed = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let completed = axum::body::to_bytes(completed.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let completed = String::from_utf8_lossy(&completed);
    assert!(completed.contains("Archives"));
    assert!(!completed.contains("Finished poster"));
    assert!(completed.contains("class=\"schedule-empty-state\""));
    assert!(completed.contains("The schedule is clear"));
    assert!(completed.contains("Recently completed"));

    let filtered = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/?q=missing")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let filtered = axum::body::to_bytes(filtered.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let filtered = String::from_utf8_lossy(&filtered);
    assert!(!filtered.contains("class=\"board-inline-status\""));
    assert!(!filtered.contains("No jobs match these filters."));
    assert!(!filtered.contains("class=\"empty\""));
    assert!(filtered.contains("class=\"schedule-empty-state\""));
    assert!(filtered.contains("Nothing matches this view"));
    assert!(!filtered.contains("class=\"timeline-row timeline-lane"));

    let shown = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/archives?q=Finished&invoice=paid&pickup=picked-up&completed_from=2020-08-20&completed_through=2020-08-20&category=Posters")
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("{error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    let shown = axum::body::to_bytes(shown.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let shown = String::from_utf8_lossy(&shown);
    assert!(shown.contains("Back to schedule"));
    assert!(shown.contains("Older completed jobs"));
    assert!(shown.contains("Paid"));
    assert!(shown.contains("Picked up"));
    assert!(shown.contains("Finished poster"));
    assert!(shown.contains("Page 1 of 1"));
    let archived_title = shown.find("Finished poster").unwrap_or_else(|| panic!());
    let archived_card_start = shown[..archived_title]
        .rfind("<article")
        .unwrap_or_else(|| panic!());
    let archived_card_end = archived_title
        + shown[archived_title..]
            .find("</article>")
            .unwrap_or_else(|| panic!());
    let archived_card = &shown[archived_card_start..archived_card_end];
    assert!(archived_card.contains("data-detail-link"));
    assert!(!archived_card.contains("data-card-detail"));
    for field in [
        "completed_from",
        "completed_through",
        "category",
        "invoice",
        "pickup",
    ] {
        assert!(shown.contains(&format!("name=\"{field}\"")));
    }

    let mismatch = app
        .oneshot(
            Request::builder()
                .uri("/archives?q=missing")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let mismatch = axum::body::to_bytes(mismatch.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let mismatch = String::from_utf8_lossy(&mismatch);
    assert!(mismatch.contains("No archived jobs found"));
    assert!(mismatch.contains("clear the filters"));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn validation_pages_preserve_values_without_job_draft_warnings() {
    let database = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repository = Repository::new(database.pool().clone());
    repository
        .ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let valid = DocumentInput {
        customer_name: "Email Customer".into(),
        issue_date: "2026-08-20".into(),
        lines: vec![LineInput {
            description: "Posters".into(),
            quantity: "1".into(),
            unit_label: "each".into(),
            unit_price: "10.00".into(),
        }],
        ..DocumentInput::default()
    }
    .validate(DocumentKind::Quote)
    .unwrap_or_else(|errors| panic!("{errors:?}"));
    let document_id = repository
        .create_document(DocumentKind::Quote, &valid)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let app = router(WebState {
        repository,
        resend: Some(
            ResendClient::with_endpoint(
                "secret".into(),
                "shop@example.test".into(),
                "http://127.0.0.1:9/emails",
                Duration::from_secs(1),
            )
            .unwrap_or_else(|error| panic!("{error}")),
        ),
        email_in_flight: Arc::new(Mutex::new(HashSet::new())),
        pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
        leptos_options: LeptosOptions::builder()
            .output_name("print-manager")
            .build(),
        storage: test_storage(),
    });

    let invalid_routes = [
            (
                "/jobs/new".to_owned(),
                "title=&description=&price=&customer_name=Preserved&quote_id=".to_owned(),
            ),
            (
                "/quotes/new".to_owned(),
                "status=draft&customer_name=&issue_date=2026-08-20&discount=0&tax_rate=0&line_description=Posters&line_quantity=1&line_unit=each&line_price=10.00".to_owned(),
            ),
            (
                "/settings".to_owned(),
                "business_name=&address=Preserved&phone=&email=&logo_path=&default_tax_rate=0&quote_prefix=Q-&invoice_prefix=INV-".to_owned(),
            ),
            (
                format!("/quotes/{document_id}/email"),
                "recipient=invalid&subject=Preserved+subject&message=Preserved+message".to_owned(),
            ),
    ];
    for (uri, body) in invalid_routes {
        let is_job = uri == "/jobs/new";
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap_or_else(|error| panic!("{error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = axum::body::to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let html = String::from_utf8_lossy(&html);
        if is_job {
            assert!(!html.contains("data-unsaved-initial=\"true\""), "{html}");
        } else {
            assert!(html.contains("data-unsaved-initial=\"true\""), "{html}");
        }
    }

    let pristine = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/jobs/new")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let pristine = axum::body::to_bytes(pristine.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(!String::from_utf8_lossy(&pristine).contains("data-unsaved-initial=\"true\""));

    let saved = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/jobs/new")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("title=Corrected&description=Corrected+job+brief&price=10.00&customer_name=Customer&quote_id="))
                    .unwrap_or_else(|error| panic!("{error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(saved.status(), StatusCode::SEE_OTHER);
    let location = saved.headers()[header::LOCATION]
        .to_str()
        .unwrap_or_else(|error| panic!("{error}"));
    let clean = app
        .oneshot(
            Request::builder()
                .uri(location)
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let clean = axum::body::to_bytes(clean.into_body(), 1_000_000)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(!String::from_utf8_lossy(&clean).contains("data-unsaved-initial=\"true\""));
}

fn multipart_file(filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = format!(
            "--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/pdf\r\n\r\n"
        )
        .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--boundary--\r\n");
    body
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn attachment_routes_enforce_streaming_limits_paths_and_lifecycle() {
    let database = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repository = Repository::new(database.pool().clone());
    repository
        .ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let job_id = repository
        .save_job(
            None,
            &JobInput {
                title: "Banner".into(),
                customer_name: "Ada".into(),
                category: "Wide format".into(),
                ..JobInput::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let storage = Storage::new(directory.path().to_path_buf(), 4);
    storage
        .prepare()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let app = router(WebState {
        repository: repository.clone(),
        resend: None,
        email_in_flight: Arc::new(Mutex::new(HashSet::new())),
        pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
        leptos_options: LeptosOptions::builder()
            .output_name("print-manager")
            .build(),
        storage: storage.clone(),
    });
    let request = |filename: &str, bytes: &[u8]| {
        Request::builder()
            .method("POST")
            .uri(format!("/jobs/{job_id}/attachments"))
            .header(
                header::CONTENT_TYPE,
                "multipart/form-data; boundary=boundary",
            )
            .body(Body::from(multipart_file(filename, bytes)))
            .unwrap_or_else(|error| panic!("{error}"))
    };

    let uploaded = app
        .clone()
        .oneshot(request("customer art.pdf", b"test"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(uploaded.status(), StatusCode::SEE_OTHER);
    let attachments = repository
        .attachments_for_job(&job_id)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(attachments[0].byte_size, 4);
    assert_eq!(
        attachments[0].sha256,
        "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
    );
    let download = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/jobs/{job_id}/attachments/{}/download",
                    attachments[0].id
                ))
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        download.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert_eq!(
        &axum::body::to_bytes(download.into_body(), 5)
            .await
            .unwrap_or_default()[..],
        b"test"
    );
    assert_eq!(
        app.clone()
            .oneshot(request("../escape.pdf", b"x"))
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.clone()
            .oneshot(request("large.pdf", b"12345"))
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(repository.complete_job(&job_id).await.unwrap_or(false));
    assert_eq!(
        app.clone()
            .oneshot(request("closed.pdf", b"x"))
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .status(),
        StatusCode::CONFLICT
    );

    let delete_uri = format!("/jobs/{job_id}/attachments/{}/delete", attachments[0].id);
    let delete = |uri: &str| {
        Request::builder()
            .method("POST")
            .uri(uri)
            .body(Body::empty())
            .unwrap_or_else(|error| panic!("{error}"))
    };
    assert_eq!(
        app.clone()
            .oneshot(delete(&delete_uri))
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .status(),
        StatusCode::CONFLICT
    );
    assert!(
        repository
            .reopen_job(&job_id, JobStage::Design.as_str())
            .await
            .unwrap_or(false)
    );
    assert_eq!(
        app.oneshot(delete(&delete_uri))
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .status(),
        StatusCode::SEE_OTHER
    );
    assert!(
        repository
            .attachments_for_job(&job_id)
            .await
            .unwrap_or_default()
            .is_empty()
    );
    assert!(storage.audit(&[]).await.unwrap_or_default().is_clean());
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn canonical_details_render_complete_no_js_action_surfaces_and_escape_values() {
    let database = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repository = Repository::new(database.pool().clone());
    repository
        .ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let job_id = repository
        .save_job(
            None,
            &JobInput {
                title: "<script>unsafe job</script>".into(),
                description: "Print the approved artwork on both sides.".into(),
                price: "250.00".into(),
                customer_name: "A & B".into(),
                customer_phone: "555-0100".into(),
                customer_email: "press@example.test".into(),
                category: "Signs".into(),
                status: "design".into(),
                notes: "Press-ready".into(),
                ..JobInput::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let tag = repository
        .create_tag("Rush", "red")
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    assert!(
        repository
            .assign_tag(&job_id, &tag.id)
            .await
            .unwrap_or(false)
    );
    let invoice_id = repository
        .create_or_open_job_invoice(&job_id)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    let attachment = repository
        .create_attachment(
            &job_id,
            &NewJobAttachment {
                original_name: "customer <art>.pdf",
                stored_name: "00000000-0000-0000-0000-000000000001",
                media_type: "application/pdf",
                byte_size: 42,
                sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());

    let quote = DocumentInput {
        customer_name: "Quote <Customer>".into(),
        customer_email: "buyer@example.test".into(),
        issue_date: "2026-08-21".into(),
        lines: vec![LineInput {
            description: "Banner".into(),
            quantity: "1".into(),
            unit_label: "each".into(),
            unit_price: "25.00".into(),
        }],
        ..DocumentInput::default()
    };
    let quote_id = repository
        .create_document(
            DocumentKind::Quote,
            &quote
                .validate(DocumentKind::Quote)
                .unwrap_or_else(|errors| panic!("{errors:?}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let empty_detail_job = "00000000-0000-0000-0000-000000000099";
    sqlx::query("INSERT INTO jobs (id,title,customer_name,category,status,notes,created_at,updated_at) VALUES (?,'Empty detail','Customer','Custom product','design','','2026-08-21T12:00:00Z','2026-08-21T12:00:00Z')")
        .bind(empty_detail_job)
        .execute(database.pool())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let app = router(WebState {
        repository: repository.clone(),
        resend: None,
        email_in_flight: Arc::new(Mutex::new(HashSet::new())),
        pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
        leptos_options: LeptosOptions::builder()
            .output_name("print-manager")
            .build(),
        storage: test_storage(),
    });
    let get_html = |uri: String| {
        let app = app.clone();
        async move {
            let response = app
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap_or_else(|error| panic!("{error}")),
                )
                .await
                .unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(response.status(), StatusCode::OK);
            String::from_utf8(
                axum::body::to_bytes(response.into_body(), 1_000_000)
                    .await
                    .unwrap_or_else(|error| panic!("{error}"))
                    .to_vec(),
            )
            .unwrap_or_else(|error| panic!("{error}"))
        }
    };
    let job = get_html(format!("/jobs/{job_id}")).await;
    for required in [
        "data-detail-content",
        "&lt;script&gt;unsafe job&lt;/script&gt;",
        "Production files",
        "Add production file",
        "Material",
        "Billing",
        &format!("/invoices/{invoice_id}/pdf"),
        &format!("/jobs/{job_id}/attachments/{}/download", attachment.id),
        "customer &lt;art&gt;.pdf",
        "Create invoice",
        "Edit job",
        "id=\"job-customer\"",
        "href=\"tel:555-0100\"",
        "href=\"mailto:press@example.test\"",
        "id=\"job-notes\"",
        "id=\"job-activity\"",
        &format!("action=\"/jobs/{job_id}/comments\""),
        "Add comment",
        "Update state",
        "Designing",
        "Press-ready",
        "id=\"job-history\"",
        "Event history",
        "<time datetime=\"",
        "data-detail-dialog",
    ] {
        if required == "Create invoice" {
            assert!(!job.contains(required));
        } else {
            assert!(job.contains(required), "missing {required}");
        }
    }
    assert!(!job.contains("<script>unsafe job</script>"));
    assert!(!job.contains(&format!("action=\"/jobs/{job_id}/card\"")));
    assert!(!job.contains("Complete job"));
    assert_eq!(
        job.matches(&format!("action=\"/jobs/{job_id}/status\""))
            .count(),
        1,
        "active drilldown must expose one lifecycle control"
    );
    let events = repository
        .job_events(&job_id)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let rendered_positions = events
        .iter()
        .map(|event| {
            job.find(&format!("data-event-kind=\"{}\"", event.kind))
                .unwrap_or(usize::MAX)
        })
        .collect::<Vec<_>>();
    assert!(
        rendered_positions.windows(2).all(|pair| pair[0] < pair[1]),
        "event history must preserve the repository's newest-first ordering"
    );
    let editor = get_html(format!("/jobs/{job_id}/edit")).await;
    for required in [
        &format!("href=\"/jobs/{job_id}\""),
        "&lt;script&gt;unsafe job&lt;/script&gt;",
        &format!("action=\"/jobs/{job_id}/edit\""),
        "Job details",
        "name=\"description\"",
        "name=\"price\"",
        "name=\"quote_id\"",
        "name=\"status\"",
        "Save job",
        "Cancel",
    ] {
        assert!(editor.contains(required), "missing {required}");
    }
    for product in ["Small print", "Banner", "Adhesive", "Poster", "Canvas"] {
        assert!(
            editor.contains(&format!("value=\"{product}\"")),
            "missing {product}"
        );
    }
    assert!(
        editor.contains("value=\"Signs\""),
        "custom product value was lost"
    );
    let empty_detail = get_html(format!("/jobs/{empty_detail_job}")).await;
    for required in [
        "id=\"job-customer\"",
        "id=\"job-notes\"",
        "No notes added.",
        "Add comment",
        "id=\"job-history\"",
        "No job events recorded yet.",
    ] {
        assert!(empty_detail.contains(required), "missing {required}");
    }
    let quote = get_html(format!("/quotes/{quote_id}")).await;
    for required in [
        "data-detail-kind=\"quote\"",
        "Download PDF",
        "Mark accepted",
        "Decline quote",
        "Edit quote",
        "Email PDF unavailable",
        "id=\"quote-contents-title\"",
        "Banner",
        "Discount",
        "Tax rate",
    ] {
        assert!(quote.contains(required), "missing {required}");
    }
    assert!(!quote.contains("Start production job"));
    assert!(!quote.contains("Create invoice"));
    let quote_editor = get_html(format!("/quotes/{quote_id}/edit")).await;
    for required in [
        "Quote details",
        ">Items</legend>",
        "Adjustments &amp; terms",
        "Save changes",
        "View quote",
        "Cancel",
    ] {
        assert!(quote_editor.contains(required), "missing {required}");
    }
    for removed_action in [
        "Mark accepted",
        "Decline",
        "Start job",
        "Create invoice",
        &format!("href=\"/quotes/{quote_id}/pdf\""),
        &format!("href=\"/quotes/{quote_id}/print\""),
    ] {
        assert!(
            !quote_editor.contains(removed_action),
            "quote editor still exposes {removed_action}"
        );
    }
    let quotes = get_html("/quotes".into()).await;
    assert!(
        quotes.contains(
            "Save estimates from customer calls so you can find them when they call back."
        )
    );
    assert!(quotes.contains("data-leptos-view=\"documents\""));
    assert!(quotes.contains("class=\"document-search\""));
    assert!(quotes.contains("name=\"q\""));
    assert!(quotes.contains("Saved estimates"));
    assert!(!quotes.contains("document-overview"));
    assert!(!quotes.contains("Working register"));
    let filtered_quotes = get_html("/quotes?q=buyer%40example.test".into()).await;
    assert!(filtered_quotes.contains("name=\"q\" value=\"buyer@example.test\""));
    assert!(filtered_quotes.contains(&format!("href=\"/quotes/{quote_id}\"")));
    assert!(!filtered_quotes.contains("No quotes match that search"));
    let filtered_invoices = get_html("/invoices?q=press%40example.test".into()).await;
    assert!(filtered_invoices.contains("name=\"q\" value=\"press@example.test\""));
    assert!(filtered_invoices.contains(&format!("href=\"/invoices/{invoice_id}/edit\"")));
    let settings = get_html("/settings".into()).await;
    for required in ["Managed logo", "action=\"/settings/logo\""] {
        assert!(settings.contains(required), "missing {required}");
    }
    assert!(!settings.contains("Reusable job labels"));
    assert!(!settings.contains("action=\"/settings/tags\""));
    assert!(!settings.contains("Legacy logo path"));
    let created_tag = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/settings/tags")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("name=Install&color_key=blue"))
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(created_tag.status(), StatusCode::SEE_OTHER);
    let archived_tag = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/settings/tags/{}/archive", tag.id))
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(archived_tag.status(), StatusCode::SEE_OTHER);
    assert!(repository.complete_job(&job_id).await.unwrap_or(false));
    let completed = get_html(format!("/jobs/{job_id}")).await;
    assert!(!completed.contains("Add production file"));
    assert!(!completed.contains("Add comment"));
    assert!(!completed.contains("Edit job"));
    assert!(!completed.contains(">Remove</button>"));
    assert!(completed.contains("Confirm pickup"));
    assert!(completed.contains("Reopen job"));
    assert!(!completed.contains("Update state"));
    assert!(!completed.contains("Active stage"));
    assert!(completed.contains("value=\"design\""));
    assert!(completed.contains("Label added: Rush"));
    assert!(
        repository
            .set_job_picked_up(&job_id, true)
            .await
            .unwrap_or(false)
    );
    let picked_up = get_html(format!("/jobs/{job_id}")).await;
    assert!(picked_up.contains("This job is closed and cannot be reopened."));
    assert!(!picked_up.contains("Confirm pickup"));
    assert!(!picked_up.contains("Reopen job"));
    assert!(!picked_up.contains("Update state"));
    let install_id = repository
        .tags(false)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|candidate| candidate.name == "Install")
        .map_or_else(|| panic!(), |candidate| candidate.id);
    let forbidden_assignment = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/jobs/{job_id}/tags/{install_id}/assign"))
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(forbidden_assignment.status(), StatusCode::CONFLICT);
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn managed_and_legacy_logo_paths_brand_pdfs_and_invalid_upload_preserves_current() {
    use std::io::Cursor;

    let database = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repository = Repository::new(database.pool().clone());
    repository
        .ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let legacy_path = directory.path().join("legacy-logo.png");
    image::DynamicImage::new_rgb8(1, 1)
        .save_with_format(&legacy_path, image::ImageFormat::Png)
        .unwrap_or_else(|error| panic!("{error}"));
    repository
        .save_logo_path(&legacy_path.to_string_lossy())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let quote = DocumentInput {
        customer_name: "Logo customer".into(),
        issue_date: "2026-08-21".into(),
        lines: vec![LineInput {
            description: "Branded print".into(),
            quantity: "1".into(),
            unit_label: "each".into(),
            unit_price: "10.00".into(),
        }],
        ..DocumentInput::default()
    };
    let quote_id = repository
        .create_document(
            DocumentKind::Quote,
            &quote
                .validate(DocumentKind::Quote)
                .unwrap_or_else(|errors| panic!("{errors:?}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let storage = Storage::new(directory.path().to_path_buf(), 1024);
    storage
        .prepare()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let app = router(WebState {
        repository: repository.clone(),
        resend: None,
        email_in_flight: Arc::new(Mutex::new(HashSet::new())),
        pdf_in_flight: Arc::new(Mutex::new(HashSet::new())),
        leptos_options: LeptosOptions::builder()
            .output_name("print-manager")
            .build(),
        storage,
    });
    let pdf_request = || {
        Request::builder()
            .uri(format!("/quotes/{quote_id}/pdf"))
            .body(Body::empty())
            .unwrap_or_else(|error| panic!("{error}"))
    };
    assert_eq!(
        app.clone()
            .oneshot(pdf_request())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .status(),
        StatusCode::OK
    );
    let legacy_preview = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/settings/logo/current")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(legacy_preview.status(), StatusCode::NOT_FOUND);
    let settings_save = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/settings")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("business_name=Shop&address=&phone=&email=&logo_path=%2Fetc%2Fpasswd&default_tax_rate=0&quote_prefix=Q-&invoice_prefix=INV-"))
                    .unwrap_or_else(|error| panic!("{error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(settings_save.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        repository
            .settings()
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .logo_path
            .as_deref(),
        Some(legacy_path.to_string_lossy().as_ref())
    );
    let request = |bytes: Vec<u8>| {
        Request::builder()
            .method("POST")
            .uri("/settings/logo")
            .header(
                header::CONTENT_TYPE,
                "multipart/form-data; boundary=boundary",
            )
            .body(Body::from(multipart_file("logo.png", &bytes)))
            .unwrap_or_else(|error| panic!("{error}"))
    };
    assert_eq!(
        app.clone()
            .oneshot(request(b"not-image".to_vec()))
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        repository
            .settings()
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .logo_path
            .as_deref(),
        Some(legacy_path.to_string_lossy().as_ref())
    );

    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(1, 1)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        app.clone()
            .oneshot(request(png.into_inner()))
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .status(),
        StatusCode::SEE_OTHER
    );
    let managed = repository
        .settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .logo_path
        .unwrap_or_default();
    assert_eq!(
        uuid::Uuid::parse_str(&managed).map(|id| id.to_string()),
        Ok(managed.clone())
    );
    assert!(
        tokio::fs::metadata(directory.path().join("branding").join(&managed))
            .await
            .is_ok()
    );
    assert_eq!(
        app.clone()
            .oneshot(pdf_request())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .status(),
        StatusCode::OK
    );
    let preview = app
        .oneshot(
            Request::builder()
                .uri("/settings/logo/current")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(preview.status(), StatusCode::OK);
    assert_eq!(preview.headers()[header::CONTENT_TYPE], "image/png");
}
