use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};

mod board;
mod schedule;

#[cfg(feature = "ssr")]
use super::shared::ValidationData;
use super::{
    documents::email_outcome_copy,
    shared::{ErrorList, StatusLabel, error_view, money},
};
use crate::product_types::STANDARD_PRODUCT_TYPES;
pub use crate::read_models::{
    ARCHIVE_PAGE_SIZE, ArchiveData, ArchiveRequest, BoardCommandRequest, BoardCommandResult,
    JobDetailData, JobFormData, JobView, JobsData, ScheduleJobRequest, ScheduleJobResult,
};
use crate::read_models::{DocumentFormData, JobEventView};
use board::archive_view;
use schedule::jobs_view;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct WorkflowFilters {
    search: String,
    category: String,
    stage: String,
    material: String,
    billing: String,
}

fn workflow_outcome_notice(
    moved: Option<&str>,
    picked_up: bool,
    sorted: bool,
    started: bool,
) -> Option<&'static str> {
    if picked_up {
        return Some("Pickup confirmed.");
    }
    if sorted {
        return Some("Card order saved.");
    }
    if started {
        return Some("Quote started as a scheduled job.");
    }
    match moved {
        Some("complete") => Some("Job completed."),
        Some("design" | "waiting" | "print" | "finish") => Some("Job updated."),
        _ => None,
    }
}

fn card_state_class(status: &str) -> &'static str {
    match status {
        "design" => "state-design",
        "waiting" => "state-waiting",
        "print" => "state-print",
        "finish" => "state-finish",
        "complete" => "state-complete",
        "draft" => "state-draft",
        "sent" => "state-sent",
        "accepted" => "state-accepted",
        "declined" => "state-declined",
        "paid" => "state-paid",
        "closed" => "state-closed",
        "picked_up_unpaid" => "state-picked-up-unpaid",
        _ => "state-unknown",
    }
}

fn billing_label(invoice_status: Option<&str>, has_invoice: bool) -> &'static str {
    if invoice_status == Some("paid") {
        "Paid"
    } else if has_invoice {
        "Unpaid"
    } else {
        "No invoice"
    }
}

#[server]
pub async fn load_jobs(
    search: String,
    category: String,
    status: String,
    material: String,
    billing: String,
) -> Result<JobsData, ServerFnError> {
    use crate::app_services::AppServices;
    let services = expect_context::<AppServices>();
    Ok(services
        .jobs_board(search, category, status, material, billing)
        .await?)
}

#[server]
pub async fn execute_board_command(
    request: BoardCommandRequest,
) -> Result<BoardCommandResult, ServerFnError> {
    use crate::app_services::AppServices;
    let services = expect_context::<AppServices>();
    services
        .execute_board_command(request)
        .await?
        .ok_or_else(|| ServerFnError::new("The board changed. Refresh and retry."))
}

#[server]
pub async fn schedule_job(request: ScheduleJobRequest) -> Result<ScheduleJobResult, ServerFnError> {
    use crate::app_services::AppServices;
    let services = expect_context::<AppServices>();
    services
        .schedule_job(request)
        .await?
        .ok_or_else(|| ServerFnError::new("The schedule changed. Refresh and retry."))
}

#[server]
pub async fn load_archives(request: ArchiveRequest) -> Result<ArchiveData, ServerFnError> {
    use crate::app_services::AppServices;
    let services = expect_context::<AppServices>();
    Ok(services.job_archive(request).await?)
}

#[server]
pub async fn load_job(
    id: Option<String>,
    quote: Option<String>,
) -> Result<JobFormData, ServerFnError> {
    use crate::app_services::AppServices;
    if let Some(ValidationData::Job(data)) = use_context::<ValidationData>() {
        return Ok(data);
    }
    let services = expect_context::<AppServices>();
    services
        .job_editor(id, quote)
        .await?
        .ok_or_else(|| ServerFnError::new("Job not found"))
}

#[server]
pub async fn load_job_detail(id: String) -> Result<JobDetailData, ServerFnError> {
    use crate::app_services::AppServices;
    let services = expect_context::<AppServices>();
    services
        .job_detail(&id)
        .await?
        .ok_or_else(|| ServerFnError::new("Job not found"))
}

#[component]

pub fn JobsPage() -> impl IntoView {
    let query = use_query_map();
    let filters = move || {
        (
            query.read().get("q").unwrap_or_default(),
            query.read().get("category").unwrap_or_default(),
            query.read().get("status").unwrap_or_default(),
            query.read().get("material").unwrap_or_default(),
            query.read().get("billing").unwrap_or_default(),
        )
    };
    let data = Resource::new(filters, |(q, category, status, material, billing)| {
        load_jobs(q, category, status, material, billing)
    });
    view! { <Suspense fallback=|| view! { <p role="status">"Loading jobs…"</p> }>{move || data.get().map(|result| match result { Ok(data) => jobs_view(
        data,
        filters(),
        query.read().get("saved").is_some(),
        workflow_outcome_notice(
            query.read().get("moved").as_deref(),
            query.read().get("picked_up").as_deref() == Some("1"),
            query.read().get("sorted").as_deref() == Some("1"),
            query.read().get("started").as_deref() == Some("1"),
        ),
    ).into_any(), Err(error) => error_view(&error.to_string()).into_any() })}</Suspense> }
}

#[component]
pub fn ArchivesPage() -> impl IntoView {
    let query = use_query_map();
    let filters = move || ArchiveRequest {
        search: query.read().get("q").unwrap_or_default(),
        invoice: query.read().get("invoice").unwrap_or_default(),
        pickup: query.read().get("pickup").unwrap_or_default(),
        completed_from: query.read().get("completed_from").unwrap_or_default(),
        completed_through: query.read().get("completed_through").unwrap_or_default(),
        category: query.read().get("category").unwrap_or_default(),
        tag_id: query.read().get("tag_id").unwrap_or_default(),
        page: query
            .read()
            .get("page")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1),
    };
    let data = Resource::new(filters, load_archives);
    view! {
        <Suspense fallback=|| view! { <p role="status">"Loading archives…"</p> }>
            {move || data.get().map(|result| match result {
                Ok(data) => archive_view(data, filters()).into_any(),
                Err(error) => error_view(&error.to_string()).into_any(),
            })}
        </Suspense>
    }
}

#[component]
pub fn JobEditorPage() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let key = move || (params.read().get("id"), query.read().get("quote"));
    let data = Resource::new(key, |(id, quote)| load_job(id, quote));
    view! { <Suspense fallback=|| view! { <p role="status">"Loading job…"</p> }>{move || data.get().map(|result| match result { Ok(data) => job_form(data, query.read().get("saved").is_some()).into_any(), Err(error) => error_view(&error.to_string()).into_any() })}</Suspense> }
}

#[component]
pub fn JobDetailPage() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let data = Resource::new(
        move || params.read().get("id").unwrap_or_default(),
        load_job_detail,
    );
    view! {
        <Suspense fallback=|| view! { <p role="status">"Loading job details…"</p> }>
            {move || data.get().map(|result| match result {
                Ok(data) => {
                    let notices = [
                        query.read().get("created").is_some().then_some("Job created."),
                        query.read().get("uploaded").is_some().then_some("Production file uploaded."),
                        query.read().get("deleted").is_some().then_some("Production file deleted."),
                        query.read().get("card_saved").is_some().then_some("Contact and notes saved."),
                        query.read().get("commented").is_some().then_some("Comment added."),
                    ].into_iter().flatten().collect();
                    job_detail_view(
                        data,
                        notices,
                        query.read().get("email"),
                        workflow_outcome_notice(
                            query.read().get("moved").as_deref(),
                            query.read().get("picked_up").as_deref() == Some("1"),
                            false,
                            false,
                        ),
                        query.read().get("created").is_some(),
                    ).into_any()
                },
                Err(error) => error_view(&error.to_string()).into_any(),
            })}
        </Suspense>
    }
}

#[allow(clippy::needless_pass_by_value)]
fn job_detail_view(
    data: JobDetailData,
    notices: Vec<&'static str>,
    email_status: Option<String>,
    workflow_notice: Option<&'static str>,
    created: bool,
) -> impl IntoView {
    let id = data.id.clone();
    let active = data.active;
    let attachments_id = id.clone();
    let invoice = data.invoice.clone();
    let events = data.events.clone();
    let status = data.status.clone();
    let billing = billing_label(
        data.invoice_status.as_deref(),
        data.invoice.as_ref().is_some(),
    );
    let state_control = job_state_control(&id, &status, active, data.picked_up);
    let customer_name = data.customer_name.clone();
    let customer_phone = data.customer_phone.clone();
    let customer_email = data.customer_email.clone();
    let notes = data.notes.clone();
    view! {
        <article class="detail-sheet job-detail" data-detail-content data-detail-kind="job">
            <header class="detail-heading job-detail-header">
                <div><a class="back-link" href="/">"← Workflow"</a><p class="eyebrow">"Production job"</p><h1>{data.title}</h1><p>{customer_name.clone()}</p></div>
                <StatusLabel value=status/>
            </header>
            {notices.into_iter().map(|notice| view! { <p class="notice" role="status">{notice}</p> }).collect_view()}
            {workflow_notice.map(|notice| view! { <p class="notice" role="status">{notice}</p> })}
            {email_status.as_deref().map(|status| {
                let copy = email_outcome_copy(status);
                if copy.success { view! { <p class="notice" role="status">{copy.message}</p> }.into_any() }
                else { view! { <div class="errors" role="alert"><p>{copy.message}</p></div> }.into_any() }
            })}
            {created.then(|| view! {
                <nav class="job-created-actions" aria-label="Job created actions">
                    <a class="button" href=format!("/jobs/{id}/edit")>"Edit job"</a>
                    <a class="button secondary-button" href="/">"Return to schedule"</a>
                    <a class="button secondary-button" href="/jobs/new">"Create another job"</a>
                </nav>
            })}
            <dl class="job-detail-facts" aria-label="Job summary">
                <div><dt>"Due"</dt><dd>{if data.due_date.is_empty() { "Not set".into() } else { data.due_date }}</dd></div>
                <div><dt>"Product"</dt><dd>{data.category}</dd></div>
                <div><dt>"Material"</dt><dd>{if data.material.is_empty() { "Not specified".into() } else { data.material }}</dd></div>
                <div><dt>"Job price"</dt><dd>{money(data.price_cents)}</dd></div>
                <div><dt>"Billing"</dt><dd>{billing}</dd></div>
            </dl>
            {state_control}
            <div class="job-detail-layout">
                <div class="job-detail-work">
                <section class="job-description" aria-labelledby="job-description">
                    <p class="eyebrow">"Customer brief"</p>
                    <h2 id="job-description">"Job description"</h2>
                    <p class="job-description-copy">{data.description}</p>
                </section>
                <section class="production-files" aria-labelledby="job-files">
                    <header class="detail-section-heading"><div><p class="eyebrow">"Job assets"</p><h2 id="job-files">"Production files"</h2><p>"Keep press-ready art, proofs, source files, and finishing references with the job."</p></div><span>{format!("{} files", data.attachments.len())}</span></header>
                    {if data.attachments.is_empty() {
                        view! { <div class="attachment-empty"><span aria-hidden="true">"↑"</span><div><strong>"No production files yet"</strong><p>"Upload the first working file so it stays connected to this job."</p></div></div> }.into_any()
                    } else {
                        view! { <ul class="attachment-list">{data.attachments.into_iter().map(|file| view! {
                            <li><span class="attachment-type" aria-hidden="true">{file_extension(&file.name)}</span><div><strong>{file.name}</strong><small>{format!("{} · {}", file.media_type, readable_file_size(file.byte_size))}</small></div><div class="actions"><a class="button secondary-button" href=format!("/jobs/{attachments_id}/attachments/{}/download", file.id)>"Download"</a>{active.then(|| view! { <form method="post" action=format!("/jobs/{attachments_id}/attachments/{}/delete", file.id) onsubmit="return confirm('Delete this production file? This cannot be undone.')"><button class="danger-button" type="submit" aria-label="Delete production file">"Delete"</button></form> })}</div></li>
                        }).collect_view()}</ul> }.into_any()
                    }}
                    {if active {
                        view! { <form class="upload-form upload-dropzone" method="post" action=format!("/jobs/{id}/attachments") enctype="multipart/form-data"><label class="file-picker"><span class="upload-mark" aria-hidden="true">"+"</span><span><strong>"Add production file"</strong><small>"PDF, artwork, office files, or packaged source · one file at a time"</small></span><input required type="file" name="file"/></label><button type="submit">"Upload to job"</button></form> }.into_any()
                    } else {
                        view! { <p class="hint">"Completed jobs are read-only; existing files remain downloadable."</p> }.into_any()
                    }}
                </section>
                {job_activity(&id, active, events)}
                </div>
                <aside class="job-detail-context" aria-label="Job context">
                    {job_customer_summary(customer_name, customer_phone, customer_email)}
                    {job_notes(notes)}
                <section class="job-billing" aria-labelledby="job-billing">
                    <header class="detail-section-heading"><div><p class="eyebrow">"Commercial handoff"</p><h2 id="job-billing">"Invoice"</h2><p>"Keep production and payment status connected without leaving the job."</p></div></header>
                    {match invoice {
                        Some(invoice) => invoice_detail(invoice),
                        None => view! { <div class="invoice-empty"><span aria-hidden="true">"$"</span><div><strong>"This job has not been invoiced"</strong><p>"Create a draft invoice using the job and customer details already on file."</p></div><form method="post" action=format!("/jobs/{id}/invoice")><button type="submit">"Create draft invoice"</button></form></div> }.into_any(),
                    }}
                </section>
                </aside>
            </div>
        </article>
    }
}

fn job_state_control(id: &str, status: &str, active: bool, picked_up: bool) -> AnyView {
    if active {
        view! {
                <section class="job-detail-actions" aria-label="Job actions">
                    <form class="active-stage-control" method="post" action=format!("/jobs/{id}/status")>
                        <input type="hidden" name="detail" value="1"/>
                        <label>"Job state"
                            <select name="status">
                                {[("design", "Designing"), ("waiting", "Waiting for customer"), ("print", "Ready to print"), ("finish", "Ready to finish"), ("complete", "Completed")].into_iter().map(|(value, label)| view! { <option value=value selected=status == value>{label}</option> }).collect_view()}
                            </select>
                        </label>
                        <button type="submit">"Update state"</button>
                    </form>
                    <a class="button secondary-button" href=format!("/jobs/{id}/edit")>"Edit job"</a>
                </section>
            }
            .into_any()
    } else if !picked_up {
        view! {
            <section class="job-detail-actions" aria-label="Job actions">
                <form method="post" action=format!("/jobs/{id}/picked-up") onsubmit="return confirm('Confirm this completed job was picked up?')"><input type="hidden" name="detail" value="1"/><button type="submit">"Confirm pickup"</button></form>
                <form class="reopen-form" method="post" action=format!("/jobs/{id}/status")><input type="hidden" name="detail" value="1"/><input type="hidden" name="status" value="design"/><button class="secondary-button" type="submit">"Reopen job"</button></form>
            </section>
        }
        .into_any()
    } else {
        view! { <section class="job-detail-actions is-read-only" aria-label="Job actions"><p><strong>"Picked up"</strong><span>"This job is closed and cannot be reopened."</span></p></section> }.into_any()
    }
}

fn job_customer_summary(name: String, phone: String, email: String) -> AnyView {
    let has_phone = !phone.is_empty();
    let has_email = !email.is_empty();
    let phone_href = format!("tel:{phone}");
    let email_href = format!("mailto:{email}");
    view! {
        <section class="job-customer" aria-labelledby="job-customer">
            <p class="eyebrow">"Customer"</p>
            <h2 id="job-customer">{name}</h2>
            <div class="job-customer-links">
                {has_phone.then(|| view! { <a href=phone_href>{phone}</a> })}
                {has_email.then(|| view! { <a href=email_href>{email}</a> })}
                {(!has_phone && !has_email).then(|| view! { <p class="hint">"No contact details provided."</p> })}
            </div>
        </section>
    }
    .into_any()
}

fn job_activity(id: &str, active: bool, events: Vec<JobEventView>) -> AnyView {
    view! {
        <section class="job-activity" aria-labelledby="job-activity">
            <header class="detail-section-heading"><div><p class="eyebrow">"Timeline"</p><h2 id="job-activity">"Activity"</h2></div></header>
            {active.then(|| view! {
                <form class="comment-form" method="post" action=format!("/jobs/{id}/comments")>
                    <label>"Add update"<textarea required maxlength="1000" name="comment" placeholder="Customer response, proof approval, or production handoff"></textarea></label>
                    <button type="submit">"Add comment"</button>
                </form>
            })}
            {job_event_history(events)}
        </section>
    }
    .into_any()
}

fn job_notes(notes: String) -> AnyView {
    view! {
        <section class="detail-notes" aria-labelledby="job-notes">
            <h2 id="job-notes">"Notes"</h2>
            {if notes.trim().is_empty() {
                view! { <p class="hint empty-state">"No notes added."</p> }.into_any()
            } else {
                view! { <p class="notes-copy">{notes}</p> }.into_any()
            }}
        </section>
    }
    .into_any()
}

fn job_event_history(events: Vec<JobEventView>) -> AnyView {
    view! {
        <div class="job-history" aria-labelledby="job-history">
            <h3 id="job-history">"Event history"</h3>
            {if events.is_empty() {
                view! { <p class="hint empty-state">"No job events recorded yet."</p> }.into_any()
            } else {
                view! { <ol class="event-history">{events.into_iter().map(|event| {
                    let display_time = readable_event_time(&event.occurred_at);
                    view! { <li data-event-kind=event.kind><p>{event.summary}</p><time datetime=event.occurred_at>{display_time}</time></li> }
                }).collect_view()}</ol> }.into_any()
            }}
        </div>
    }.into_any()
}

fn invoice_detail(invoice: DocumentFormData) -> AnyView {
    let invoice_id = invoice.id.unwrap_or_default();
    let number = invoice.number.unwrap_or_default();
    let invoice_status = invoice.status.clone();
    view! {
        <div class="job-invoice-summary"><div><span class="invoice-number">{format!("Invoice {number}")}</span><StatusLabel value=invoice_status/></div><dl><div><dt>"Customer"</dt><dd>{invoice.customer_name.clone()}</dd></div><div><dt>"Due"</dt><dd>{if invoice.date.is_empty() { "Not set".into() } else { invoice.date.clone() }}</dd></div></dl><div class="actions"><a class="button" href=format!("/invoices/{invoice_id}/edit")>"Open invoice"</a><a class="button secondary-button" href=format!("/invoices/{invoice_id}/pdf") data-pdf-action="download" aria-describedby="job-pdf-status">"PDF"</a><a class="button secondary-button" href=format!("/invoices/{invoice_id}/print") target="_blank" rel="noopener" data-pdf-action="print" aria-describedby="job-pdf-status">"Print"</a></div></div>
        <p id="job-pdf-status" class="pdf-action-status" role="status" aria-live="polite" hidden></p>
    }.into_any()
}

fn readable_file_size(bytes: i64) -> String {
    let bytes = u64::try_from(bytes).unwrap_or_default();
    if bytes >= 1024 * 1024 {
        one_decimal_unit(bytes, 1024 * 1024, "MB")
    } else if bytes >= 1024 {
        one_decimal_unit(bytes, 1024, "KB")
    } else {
        format!("{bytes} B")
    }
}

fn one_decimal_unit(bytes: u64, unit: u64, label: &str) -> String {
    let whole = bytes / unit;
    let decimal = bytes % unit * 10 / unit;
    format!("{whole}.{decimal} {label}")
}

fn file_extension(name: &str) -> String {
    name.rsplit_once('.')
        .map_or("FILE", |(_, extension)| extension)
        .chars()
        .take(4)
        .collect::<String>()
        .to_uppercase()
}

fn job_form(data: JobFormData, saved_notice: bool) -> impl IntoView {
    let editing = data.id.is_some();
    let heading = if editing {
        data.title.clone()
    } else {
        "New job".into()
    };
    let return_href = data
        .id
        .as_ref()
        .map_or_else(|| "/".into(), |id| format!("/jobs/{id}"));
    let return_label = if editing {
        "← Job details"
    } else {
        "← Workflow"
    };
    let action = data
        .id
        .as_ref()
        .map_or_else(|| "/jobs/new".into(), |id| format!("/jobs/{id}/edit"));
    let quote_link = data
        .quote_id
        .as_ref()
        .filter(|quote_id| !quote_id.trim().is_empty())
        .map(|quote_id| {
        let label = data
            .quote_number
            .as_deref()
            .unwrap_or(quote_id.as_str())
            .to_owned();
        view! {
            <p class="source-quote-link">"Source quote: "<a href=format!("/quotes/{quote_id}")>{label}</a></p>
        }
    });
    view! {
        <div class="job-editor-page">
            <header class="job-editor-heading">
                <div>
                    <a class="back-link" href=return_href.clone()>{return_label}</a>
                    <p class="eyebrow">"Production job"</p>
                    <h1>{heading}</h1>
                    <p class="save-state">{if editing { "Update the job brief, price, or customer contact details." } else { "Record the work, price, and any customer contact details." }}</p>
                </div>
            </header>
            {saved_notice.then(|| view! { <p class="notice" role="status">"Job saved."</p> })}
            <ErrorList errors=data.errors.clone()/>
            {quote_link}
            <form method="post" action=action class="job-editor">
                <input type="hidden" name="quote_id" value=data.quote_id.clone().unwrap_or_default()/>
                {editing.then(|| view! { <input type="hidden" name="status" value=data.status/> })}
                <fieldset class="job-editor-section job-details">
                    <legend>"Job details"</legend>
                    <div class="job-editor-fields">
                        <label class="wide" for="job-title">"Job title"<input id="job-title" required maxlength="200" name="title" value=data.title placeholder="e.g. Lobby window graphics"/></label>
                        <label class="wide" for="job-description">"Job description"<textarea id="job-description" required maxlength="5000" name="description" rows="6" placeholder="What does the customer need, and what specifications should the shop follow?">{data.description}</textarea><span class="hint">"Include the customer's request and any relevant specifications."</span></label>
                        <label class="price-field" for="job-price">"Job price"<input id="job-price" required type="number" min="0" step="0.01" inputmode="decimal" maxlength="20" name="price" value=data.price placeholder="0.00"/><span class="hint">"Enter a non-negative amount in dollars."</span></label>
                        {editing.then(|| view! {
                            <label>"Product type"<input maxlength="100" name="category" list="job-product-types" value=data.category/><span class="hint">"Optional operational detail."</span></label>
                            <label>"Material"<input maxlength="100" name="material" placeholder="e.g. Vinyl, foam board, cardstock" value=data.material/></label>
                            <label>"Due date"<input type="date" name="due_date" value=data.due_date/><span class="hint">"Blank keeps the existing schedule."</span></label>
                            <label class="wide">"Internal notes"<textarea maxlength="5000" name="notes" rows="4">{data.notes}</textarea></label>
                        })}
                    </div>
                </fieldset>
                <fieldset class="job-editor-section customer-details">
                    <legend>"Customer contact"</legend>
                    <p class="section-help">"Optional. Leave any field blank when the customer did not provide it."</p>
                    <div class="job-editor-fields">
                        <label class="wide" for="customer-name">"Customer name"<input id="customer-name" type="text" maxlength="200" name="customer_name" autocomplete="name" value=data.customer_name/></label>
                        <label for="customer-phone">"Phone"<input id="customer-phone" type="text" maxlength="100" name="customer_phone" autocomplete="tel" value=data.customer_phone/></label>
                        <label for="customer-email">"Email"<input id="customer-email" type="text" maxlength="320" name="customer_email" autocomplete="email" value=data.customer_email/></label>
                    </div>
                </fieldset>
                {editing.then(|| view! { <datalist id="job-product-types">{STANDARD_PRODUCT_TYPES.into_iter().map(|category| view! { <option value=category></option> }).collect_view()}</datalist> })}
                <div class="job-editor-actions">
                    <div><strong>{if editing { "Save job changes" } else { "Create job" }}</strong><small>{if editing { "Operational workflow controls remain on the job." } else { "The job starts in Design with the shop's standard turnaround." }}</small></div>
                    <div class="actions"><a class="button secondary-button" href=return_href>"Cancel"</a><button type="submit">{if editing { "Save job" } else { "Create job" }}</button></div>
                </div>
            </form>
        </div>
    }
}

fn readable_event_time(value: &str) -> String {
    let Some((date, time_and_zone)) = value.split_once('T') else {
        return value.to_owned();
    };
    let mut date_parts = date.split('-');
    let (Some(year), Some(month), Some(day), None) = (
        date_parts.next(),
        date_parts.next(),
        date_parts.next(),
        date_parts.next(),
    ) else {
        return value.to_owned();
    };
    let Ok(month_number) = month.parse::<usize>() else {
        return value.to_owned();
    };
    let Some(month_name) = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .get(month_number.saturating_sub(1)) else {
        return value.to_owned();
    };
    let zone_start = time_and_zone.find(['Z', '+']).or_else(|| {
        time_and_zone
            .get(1..)
            .and_then(|rest| rest.find('-').map(|index| index + 1))
    });
    let Some(zone_start) = zone_start else {
        return value.to_owned();
    };
    let (clock, zone) = time_and_zone.split_at(zone_start);
    let clock = clock.split('.').next().unwrap_or(clock);
    let mut clock_parts = clock.split(':');
    let (Some(hour), Some(minute)) = (clock_parts.next(), clock_parts.next()) else {
        return value.to_owned();
    };
    let Ok(hour) = hour.parse::<u8>() else {
        return value.to_owned();
    };
    if hour > 23 || minute.len() != 2 || !minute.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.to_owned();
    }
    let day = day.trim_start_matches('0');
    let period = if hour < 12 { "AM" } else { "PM" };
    let display_hour = match hour % 12 {
        0 => 12,
        hour => hour,
    };
    let zone = if zone == "Z" {
        "UTC".to_owned()
    } else {
        format!("UTC{zone}")
    };
    format!("{month_name} {day}, {year} at {display_hour}:{minute} {period} {zone}")
}

#[cfg(test)]
mod tests;
