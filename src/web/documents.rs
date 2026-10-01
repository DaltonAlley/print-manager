use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use super::{WebState, error_page, not_found, validation_page};
use axum::{
    Router,
    body::Bytes,
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};

use crate::{
    calculation::Money,
    domain::{DocumentInput, DocumentKind, LineInput},
    email::{EmailError, EmailInput, MAX_EMAIL_BODY_BYTES},
    pdf::{self, PdfDocumentView, PdfLine},
    repository::{FullDocument, Settings},
    storage::Storage,
    ui::{
        documents::{DocumentFormData, EmailFormData, LineForm},
        shared::ValidationData,
    },
};

pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route("/quotes/new", post(create_quote))
        .route("/quotes/{id}/edit", post(update_quote))
        .route("/quotes/{id}/pdf", get(quote_pdf))
        .route("/quotes/{id}/print", get(quote_print))
        .route("/quotes/{id}/email", post(quote_email))
        .route("/quotes/{id}/accepted", post(accept_quote))
        .route("/quotes/{id}/create-invoice", post(quote_invoice))
        .route("/quotes/{id}/declined", post(decline_quote))
        .route("/invoices/new", post(create_invoice))
        .route("/invoices/{id}/edit", post(update_invoice))
        .route("/invoices/{id}/pdf", get(invoice_pdf))
        .route("/invoices/{id}/print", get(invoice_print))
        .route("/invoices/{id}/email", post(invoice_email))
        .route("/invoices/{id}/paid", post(mark_paid))
}

pub(super) struct RequestFlight {
    active: Arc<Mutex<HashSet<String>>>,
    key: String,
}

impl Drop for RequestFlight {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.key);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FlightError {
    Duplicate,
    Unavailable,
}

pub(super) fn begin_flight(
    active: &Arc<Mutex<HashSet<String>>>,
    key: String,
) -> Result<RequestFlight, FlightError> {
    let mut requests = active.lock().map_err(|_| FlightError::Unavailable)?;
    if !requests.insert(key.clone()) {
        return Err(FlightError::Duplicate);
    }
    drop(requests);
    Ok(RequestFlight {
        active: active.clone(),
        key,
    })
}

#[derive(Debug)]
pub(super) enum EmailAttemptError {
    NotFound,
    Repository,
    Pdf,
    DeliveryUnknown,
    Send(EmailError),
}

pub(super) async fn create_quote(
    OriginalUri(uri): OriginalUri,
    State(state): State<WebState>,
    body: Bytes,
) -> Response {
    save_document_response(
        &state,
        uri,
        DocumentKind::Quote,
        None,
        parse_document(&body),
    )
    .await
}
pub(super) async fn create_invoice(
    OriginalUri(uri): OriginalUri,
    State(state): State<WebState>,
    body: Bytes,
) -> Response {
    save_document_response(
        &state,
        uri,
        DocumentKind::Invoice,
        None,
        parse_document(&body),
    )
    .await
}
pub(super) async fn update_quote(
    OriginalUri(uri): OriginalUri,
    State(state): State<WebState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    save_document_response(
        &state,
        uri,
        DocumentKind::Quote,
        Some(&id),
        parse_document(&body),
    )
    .await
}
pub(super) async fn update_invoice(
    OriginalUri(uri): OriginalUri,
    State(state): State<WebState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    save_document_response(
        &state,
        uri,
        DocumentKind::Invoice,
        Some(&id),
        parse_document(&body),
    )
    .await
}
pub(super) async fn save_document_response(
    state: &WebState,
    uri: Uri,
    kind: DocumentKind,
    id: Option<&str>,
    input: DocumentInput,
) -> Response {
    let terminal = match kind {
        DocumentKind::Quote => "declined",
        DocumentKind::Invoice => "paid",
    };
    let terminal_transition = if input.status == terminal {
        match id {
            Some(id) => state
                .repository
                .document(id, kind)
                .await
                .ok()
                .flatten()
                .is_none_or(|document| document.document.status != terminal),
            None => true,
        }
    } else {
        false
    };
    let valid = match input.validate(kind).and_then(|valid| {
        if terminal_transition {
            Err(vec![format!(
                "Use the confirmed action to mark this {} {terminal}.",
                kind.as_str()
            )])
        } else {
            Ok(valid)
        }
    }) {
        Ok(v) => v,
        Err(errors) => {
            let number = if let Some(id) = id {
                state
                    .repository
                    .document(id, kind)
                    .await
                    .ok()
                    .flatten()
                    .map(|document| document.document.number)
            } else {
                None
            };
            return validation_page(
                state,
                uri,
                ValidationData::Document(Box::new(document_form_data(
                    kind,
                    id,
                    number,
                    input,
                    errors,
                    state.resend_configured(),
                ))),
            )
            .await;
        }
    };
    let result = match id {
        Some(id) => state
            .repository
            .update_document(id, kind, &valid)
            .await
            .map(|ok| ok.then(|| id.to_owned())),
        None => state
            .repository
            .create_document(kind, &valid)
            .await
            .map(Some),
    };
    match result {
        Ok(Some(id)) => {
            Redirect::to(&format!("/{}/{}/edit?saved=1", kind.plural(), id)).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => error_page(&e.to_string()),
    }
}

pub(super) fn parse_document(body: &[u8]) -> DocumentInput {
    let pairs = url::form_urlencoded::parse(body);
    let mut values: HashMap<String, Vec<String>> = HashMap::new();
    for (k, v) in pairs {
        values
            .entry(k.into_owned())
            .or_default()
            .push(v.into_owned());
    }
    let descriptions = values.remove("line_description").unwrap_or_default();
    let quantities = values.remove("line_quantity").unwrap_or_default();
    let units = values.remove("line_unit").unwrap_or_default();
    let prices = values.remove("line_price").unwrap_or_default();
    let lines = (0..descriptions.len())
        .map(|i| LineInput {
            description: descriptions.get(i).cloned().unwrap_or_default(),
            quantity: quantities.get(i).cloned().unwrap_or_default(),
            unit_label: units.get(i).cloned().unwrap_or_default(),
            unit_price: prices.get(i).cloned().unwrap_or_default(),
        })
        .filter(|line| {
            ![
                &line.description,
                &line.quantity,
                &line.unit_label,
                &line.unit_price,
            ]
            .iter()
            .all(|value| value.trim().is_empty())
        })
        .collect();
    let one = |name: &str| {
        values
            .get(name)
            .and_then(|v| v.first())
            .cloned()
            .unwrap_or_default()
    };
    DocumentInput {
        status: one("status"),
        customer_name: one("customer_name"),
        customer_email: one("customer_email"),
        customer_phone: one("customer_phone"),
        billing_address: one("billing_address"),
        issue_date: one("issue_date"),
        expiry_or_due_date: one("expiry_or_due_date"),
        discount: one("discount"),
        tax_rate: one("tax_rate"),
        notes: one("notes"),
        terms: one("terms"),
        lines,
    }
}

pub(super) fn document_form_data(
    kind: DocumentKind,
    id: Option<&str>,
    number: Option<String>,
    input: DocumentInput,
    errors: Vec<String>,
    resend_configured: bool,
) -> DocumentFormData {
    DocumentFormData {
        id: id.map(str::to_owned),
        number,
        kind: kind.as_str().to_owned(),
        status: input.status,
        customer_name: input.customer_name,
        customer_email: input.customer_email,
        customer_phone: input.customer_phone,
        billing_address: input.billing_address,
        issue_date: input.issue_date,
        date: input.expiry_or_due_date,
        discount: input.discount,
        tax_rate: input.tax_rate,
        notes: input.notes,
        terms: input.terms,
        lines: input
            .lines
            .into_iter()
            .map(|line| LineForm {
                description: line.description,
                quantity: line.quantity,
                unit_label: line.unit_label,
                unit_price: line.unit_price,
            })
            .collect(),
        resend_configured,
        errors,
        email: None,
    }
}

pub(super) async fn quote_invoice(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Response {
    match state.repository.copy_quote_to_invoice(&id).await {
        Ok(Some(invoice)) => {
            Redirect::to(&format!("/invoices/{invoice}/edit?saved=1")).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => error_page(&e.to_string()),
    }
}
pub(super) async fn mark_paid(State(state): State<WebState>, Path(id): Path<String>) -> Response {
    match state.repository.mark_paid(&id).await {
        Ok(true) => Redirect::to(&format!("/invoices/{id}/edit?saved=1")).into_response(),
        Ok(false) => not_found(),
        Err(e) => error_page(&e.to_string()),
    }
}
pub(super) async fn accept_quote(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Response {
    match state.repository.accept_quote(&id).await {
        Ok(true) => Redirect::to(&format!("/quotes/{id}?saved=1")).into_response(),
        Ok(false) => not_found(),
        Err(e) => error_page(&e.to_string()),
    }
}
pub(super) async fn decline_quote(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Response {
    match state.repository.decline_quote(&id).await {
        Ok(true) => Redirect::to(&format!("/quotes/{id}?saved=1")).into_response(),
        Ok(false) => not_found(),
        Err(e) => error_page(&e.to_string()),
    }
}
pub(super) async fn quote_pdf(State(state): State<WebState>, Path(id): Path<String>) -> Response {
    pdf_response(&state, &id, DocumentKind::Quote, true).await
}
pub(super) async fn quote_print(State(state): State<WebState>, Path(id): Path<String>) -> Response {
    pdf_response(&state, &id, DocumentKind::Quote, false).await
}
pub(super) async fn invoice_pdf(State(state): State<WebState>, Path(id): Path<String>) -> Response {
    pdf_response(&state, &id, DocumentKind::Invoice, true).await
}
pub(super) async fn invoice_print(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Response {
    pdf_response(&state, &id, DocumentKind::Invoice, false).await
}

pub(super) async fn quote_email(
    State(state): State<WebState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let uri = match format!("/quotes/{id}").parse::<Uri>() {
        Ok(uri) => uri,
        Err(error) => return error_page(&error.to_string()),
    };
    email_response(&state, uri, &id, DocumentKind::Quote, &body).await
}

pub(super) async fn invoice_email(
    State(state): State<WebState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let uri = match format!("/invoices/{id}/edit").parse::<Uri>() {
        Ok(uri) => uri,
        Err(error) => return error_page(&error.to_string()),
    };
    email_response(&state, uri, &id, DocumentKind::Invoice, &body).await
}

pub(super) fn document_email_destination(kind: DocumentKind, id: &str, status: &str) -> String {
    match kind {
        DocumentKind::Quote => format!("/quotes/{id}?email={status}"),
        DocumentKind::Invoice => format!("/invoices/{id}/edit?email={status}"),
    }
}

pub(super) async fn email_response(
    state: &WebState,
    uri: Uri,
    id: &str,
    kind: DocumentKind,
    body: &[u8],
) -> Response {
    let requested_job = url::form_urlencoded::parse(body)
        .find_map(|(key, value)| (key == "job_id").then(|| value.into_owned()));
    let return_job = if let Some(job_id) = requested_job {
        match state.repository.job(&job_id).await {
            Ok(Some(job)) if job.invoice_id.as_deref() == Some(id) => Some(job_id),
            _ => None,
        }
    } else {
        None
    };
    let redirect = |status: &str| {
        let destination = return_job.as_ref().map_or_else(
            || document_email_destination(kind, id, status),
            |job_id| format!("/jobs/{job_id}?email={status}"),
        );
        Redirect::to(&destination).into_response()
    };
    let Some(resend) = &state.resend else {
        return redirect("not-configured");
    };
    if body.len() > MAX_EMAIL_BODY_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            "Email form is too large. Shorten the message and retry; the saved document is safe.",
        )
            .into_response();
    }
    let input = parse_email(body);
    if let Err(error) = input.validate() {
        let document = match state.repository.document(id, kind).await {
            Ok(Some(document)) => document,
            Ok(None) => return not_found(),
            Err(error) => return error_page(&error.to_string()),
        };
        let mut data = document_form_data(
            kind,
            Some(id),
            Some(document.document.number.clone()),
            document.to_input(),
            Vec::new(),
            state.resend_configured(),
        );
        data.email = Some(EmailFormData {
            recipient: input.recipient,
            subject: input.subject,
            message: input.message,
            errors: vec![error.to_string()],
        });
        return validation_page(state, uri, ValidationData::Document(Box::new(data))).await;
    }
    let flight_key = format!("{}:{id}", kind.as_str());
    let _flight = match begin_flight(&state.email_in_flight, flight_key) {
        Ok(flight) => flight,
        Err(FlightError::Duplicate) => {
            return (StatusCode::CONFLICT, "An email for this document is already being sent. Wait for it to finish before retrying.").into_response();
        }
        Err(FlightError::Unavailable) => return redirect("failed"),
    };

    let result = async {
        let document = state
            .repository
            .document(id, kind)
            .await
            .map_err(|_| EmailAttemptError::Repository)?
            .ok_or(EmailAttemptError::NotFound)?;
        let settings = state
            .repository
            .settings()
            .await
            .map_err(|_| EmailAttemptError::Repository)?;
        let rendered = pdf::render_document(&pdf_view(&document, &settings, kind, &state.storage))
            .map_err(|_| EmailAttemptError::Pdf)?;
        let filename = format!(
            "{}.pdf",
            document.document.number.replace(['\r', '\n', '"'], "")
        );
        let provider_id = resend
            .send_pdf(&input, &filename, &rendered.bytes)
            .await
            .map_err(EmailAttemptError::Send)?;
        if !state
            .repository
            .record_email_success(id, kind, &provider_id)
            .await
            .map_err(|_| EmailAttemptError::DeliveryUnknown)?
        {
            return Err(EmailAttemptError::DeliveryUnknown);
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => redirect("sent"),
        Err(EmailAttemptError::NotFound) => not_found(),
        Err(error) => redirect(email_error_status(&error)),
    }
}

pub(super) fn email_error_status(error: &EmailAttemptError) -> &'static str {
    match error {
        EmailAttemptError::Repository => "repository",
        EmailAttemptError::Pdf => "pdf",
        EmailAttemptError::DeliveryUnknown
        | EmailAttemptError::Send(EmailError::DeliveryUnknown) => "unknown",
        EmailAttemptError::NotFound | EmailAttemptError::Send(EmailError::InvalidInput(_)) => {
            "invalid"
        }
        EmailAttemptError::Send(EmailError::AttachmentTooLarge) => "too-large",
        EmailAttemptError::Send(EmailError::ProviderRejected) => "rejected",
        EmailAttemptError::Send(EmailError::Transport) => "network",
        EmailAttemptError::Send(EmailError::InvalidConfirmation) => "confirmation",
    }
}

pub(super) fn parse_email(body: &[u8]) -> EmailInput {
    let mut values = HashMap::new();
    for (key, value) in url::form_urlencoded::parse(body) {
        values.entry(key.into_owned()).or_insert(value.into_owned());
    }
    EmailInput {
        recipient: values.remove("recipient").unwrap_or_default(),
        subject: values.remove("subject").unwrap_or_default(),
        message: values.remove("message").unwrap_or_default(),
    }
}
pub(super) async fn pdf_response(
    state: &WebState,
    id: &str,
    kind: DocumentKind,
    attachment: bool,
) -> Response {
    let flight_key = format!("{}:{id}", kind.as_str());
    let _flight = match begin_flight(&state.pdf_in_flight, flight_key) {
        Ok(flight) => flight,
        Err(FlightError::Duplicate) => {
            return private_text_response(
                StatusCode::CONFLICT,
                "A PDF for this document is already being prepared. Wait for it to finish before retrying.",
            );
        }
        Err(FlightError::Unavailable) => {
            return private_text_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "PDF preparation is temporarily unavailable. Your saved document is safe; retry shortly.",
            );
        }
    };
    let document = match state.repository.document(id, kind).await {
        Ok(Some(d)) => d,
        Ok(None) => return not_found(),
        Err(e) => return error_page(&e.to_string()),
    };
    let settings = match state.repository.settings().await {
        Ok(s) => s,
        Err(e) => return error_page(&e.to_string()),
    };
    let view = pdf_view(&document, &settings, kind, &state.storage);
    let rendered = match pdf::render_document(&view) {
        Ok(r) => r,
        Err(e) => return error_page(&e.to_string()),
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/pdf"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    let disposition = format!(
        "{}; filename=\"{}.pdf\"",
        if attachment { "attachment" } else { "inline" },
        document.document.number.replace(['\r', '\n', '\"'], "")
    );
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    (headers, rendered.bytes).into_response()
}

pub(super) fn private_text_response(status: StatusCode, message: &'static str) -> Response {
    let mut response = (status, message).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}
pub(super) fn pdf_view(
    doc: &FullDocument,
    settings: &Settings,
    kind: DocumentKind,
    storage: &Storage,
) -> PdfDocumentView {
    PdfDocumentView {
        kind,
        number: doc.document.number.clone(),
        status: doc.document.status.clone(),
        business_name: settings.business_name.clone(),
        business_address: settings.address.clone(),
        business_phone: settings.phone.clone(),
        business_email: settings.email.clone(),
        logo_path: settings.logo_path.as_deref().map(|path| {
            storage
                .logo_path_for_pdf(path)
                .to_string_lossy()
                .into_owned()
        }),
        customer_name: doc.document.customer_name.clone(),
        customer_email: doc.document.customer_email.clone(),
        customer_phone: doc.document.customer_phone.clone(),
        billing_address: doc.document.billing_address.clone(),
        issue_date: doc.document.issue_date.clone(),
        expiry_or_due_date: doc.document.expiry_or_due_date.clone(),
        lines: doc
            .lines
            .iter()
            .map(|line| PdfLine {
                description: line.description.clone(),
                quantity: line.quantity.parse().unwrap_or_default(),
                unit_label: line.unit_label.clone(),
                unit_price: Money::from_cents(line.unit_price_cents).unwrap_or(Money::ZERO),
            })
            .collect(),
        discount: Money::from_cents(doc.document.discount_cents).unwrap_or(Money::ZERO),
        tax_basis_points: u32::try_from(doc.document.tax_basis_points).unwrap_or(0),
        notes: doc.document.notes.clone(),
        terms: doc.document.terms.clone(),
    }
}
