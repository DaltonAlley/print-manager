#[cfg(feature = "ssr")]
use super::shared::ValidationData;
use super::shared::{EditorStatusOptions, ErrorList, StatusLabel, error_view, money};
pub use crate::read_models::{DocumentFormData, DocumentSummary, EmailFormData, LineForm};
use leptos::prelude::*;
use leptos_router::hooks::{use_location, use_params_map, use_query_map};

#[server]
pub async fn load_documents(
    kind: String,
    search: String,
) -> Result<Vec<DocumentSummary>, ServerFnError> {
    use crate::app_services::AppServices;
    let kind = parse_kind(&kind)?;
    let services = expect_context::<AppServices>();
    Ok(services.document_list(kind, search).await?)
}

#[server]
pub async fn load_document(
    kind: String,
    id: Option<String>,
) -> Result<DocumentFormData, ServerFnError> {
    use crate::app_services::AppServices;
    if let Some(ValidationData::Document(data)) = use_context::<ValidationData>() {
        return Ok(*data);
    }
    let parsed_kind = parse_kind(&kind)?;
    let services = expect_context::<AppServices>();
    services
        .document_editor(parsed_kind, id)
        .await?
        .ok_or_else(|| ServerFnError::new("Document not found"))
}

#[cfg(feature = "ssr")]
fn parse_kind(value: &str) -> Result<crate::domain::DocumentKind, ServerFnError> {
    match value {
        "quote" => Ok(crate::domain::DocumentKind::Quote),
        "invoice" => Ok(crate::domain::DocumentKind::Invoice),
        _ => Err(ServerFnError::new("Unknown document kind")),
    }
}

#[component]
pub fn QuotesPage() -> impl IntoView {
    document_list_page("quote")
}
#[component]
pub fn InvoicesPage() -> impl IntoView {
    document_list_page("invoice")
}

fn document_list_page(kind: &'static str) -> impl IntoView {
    let query = use_query_map();
    let filters = move || (kind.to_string(), query.read().get("q").unwrap_or_default());
    let data = Resource::new(filters, |(kind, search)| load_documents(kind, search));
    view! { <Suspense fallback=|| view! { <p role="status">"Loading documents…"</p> }>{move || data.get().map(|result| match result { Ok(docs) => { let search = filters().1; document_list_view(kind, docs, &search, query.read().get("saved").is_some()).into_any() }, Err(error) => error_view(&error.to_string()).into_any() })}</Suspense> }
}

fn document_list_view(
    kind: &str,
    docs: Vec<DocumentSummary>,
    search: &str,
    saved: bool,
) -> impl IntoView {
    let label = if kind == "quote" { "Quote" } else { "Invoice" };
    let plural = if kind == "quote" {
        "quotes"
    } else {
        "invoices"
    };
    let create = format!("/{plural}/new");
    let document_count = docs.len();
    let completed_count = docs
        .iter()
        .filter(|document| document.status == "paid")
        .count();
    let active_count = docs
        .iter()
        .filter(|document| matches!(document.status.as_str(), "draft" | "sent"))
        .count();
    let total_cents = docs.iter().fold(0_i64, |total, document| {
        total.saturating_add(document.total_cents)
    });
    let search_id = format!("{kind}-search");
    let search_placeholder = if kind == "quote" {
        "Quote number, customer, phone, or print item"
    } else {
        "Invoice number, customer, phone, or print item"
    };
    let search_is_empty = search.trim().is_empty();
    let count_label = if search_is_empty { "total" } else { "matching" };
    view! {
        <div class="document-workspace">
            <header class="document-index-heading">
                <div>{(kind == "invoice").then(|| view! { <p class="eyebrow">"Accounts receivable"</p> })}<h1>{format!("{label}s")}</h1><p>{if kind == "quote" { "Save estimates from customer calls so you can find them when they call back." } else { "Create invoices from print jobs and print them when a customer needs a copy." }}</p></div>
                <a class="button" href=create.clone()><span aria-hidden="true">"+"</span>{format!("New {kind}")}</a>
            </header>
            {saved.then(|| view! { <p class="notice" role="status">{format!("{label} saved.")}</p> })}
            {(kind != "quote").then(|| view! { <section class="document-overview" aria-label="Invoice summary"><div><span>"Records"</span><strong>{document_count}</strong></div><div><span>"Outstanding"</span><strong>{active_count}</strong></div><div><span>"Paid"</span><strong>{completed_count}</strong></div><div class="document-overview-value"><span>"Total value"</span><strong>{money(total_cents)}</strong></div></section> })}
            <section class="document-register" aria-label=format!("All {plural}")>
                <header><div><p class="eyebrow">{if kind == "quote" { "Saved estimates" } else { "Invoice register" }}</p><h2 id="document-register-title">{format!("All {plural}")}</h2></div><span>{format!("{document_count} {count_label}")}</span></header>
                <form class="document-search" method="get" action=format!("/{plural}") role="search">
                    <label for=search_id.clone()>{format!("Find {plural}")}</label>
                    <div class="document-search-controls"><input id=search_id.clone() type="search" name="q" value=search.to_string() placeholder=search_placeholder autocomplete="off"/><button class="secondary-button" type="submit">"Search"</button>{(!search_is_empty).then(|| view! { <a class="button secondary-button" href=format!("/{plural}")>"Clear"</a> })}</div>
                </form>
                {if docs.is_empty() { view! { <div class="document-empty"><span class="document-empty-mark" aria-hidden="true">{if kind == "quote" { "Q" } else { "$" }}</span><div><h3>{if search_is_empty { format!("No {kind}s yet") } else { format!("No {kind}s match that search") }}</h3><p>{if search_is_empty { if kind == "quote" { "Create the first estimate from the New quote action above." } else { "Create an invoice directly or from a print job or accepted quote." } } else { "Try a document number, customer name, phone number, or print item." }}</p></div></div> }.into_any() } else { view! { <div class="document-table-wrap"><table class="document-table" data-leptos-view="documents"><thead><tr><th>"Number"</th><th>"Customer"</th><th>"Total"</th><th>"Status"</th><th>{if kind == "quote" { "Expires" } else { "Due" }}</th><th>"Updated"</th></tr></thead><tbody>{docs.into_iter().map(|doc| { let destination = if kind == "quote" { format!("/{plural}/{}", doc.id) } else { format!("/{plural}/{}/edit", doc.id) }; view! { <tr><th data-label="Number"><a href=destination data-detail-link=kind == "quote">{doc.number}</a></th><td data-label="Customer">{doc.customer_name}</td><td data-label="Total" class="document-total">{money(doc.total_cents)}</td><td data-label="Status"><StatusLabel value=doc.status/></td><td data-label=if kind == "quote" { "Expires" } else { "Due" }>{if doc.date.is_empty() { "Not set".into() } else { doc.date }}</td><td data-label="Updated" class="document-updated">{updated_date(&doc.updated_at)}</td></tr> }}).collect_view()}</tbody></table></div> }.into_any() }}
            </section>
        </div>
    }
}

fn updated_date(value: &str) -> String {
    let date = value.split('T').next().unwrap_or(value);
    let mut parts = date.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return value.to_owned();
    };
    let Ok(month) = month.parse::<usize>() else {
        return value.to_owned();
    };
    let Some(month) = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .get(month.saturating_sub(1)) else {
        return value.to_owned();
    };
    format!("{month} {}, {year}", day.trim_start_matches('0'))
}

#[component]
pub fn QuoteDetailPage() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let key = move || ("quote".to_string(), params.read().get("id"));
    let data = Resource::new(key, |(kind, id)| load_document(kind, id));
    view! { <Suspense fallback=|| view! { <p role="status">"Loading quote details…"</p> }>{move || data.get().map(|result| match result { Ok(data) => quote_detail_view(data, query.read().get("email")).into_any(), Err(error) => error_view(&error.to_string()).into_any() })}</Suspense> }
}

#[allow(clippy::needless_pass_by_value)]
fn quote_detail_view(data: DocumentFormData, email_status: Option<String>) -> impl IntoView {
    let id = data.id.clone().unwrap_or_default();
    let number = data.number.clone().unwrap_or_default();
    let email_data = data.email.clone().unwrap_or_else(|| EmailFormData {
        recipient: data.customer_email.clone(),
        subject: format!("Quote {number}"),
        ..EmailFormData::default()
    });
    let is_accepted = data.status == "accepted";
    let is_declined = data.status == "declined";
    view! {
        <article class="detail-sheet document-detail" data-detail-content data-detail-kind="quote">
            <header class="detail-heading"><div><p class="eyebrow">"Quote workspace"</p><h1>{format!("Quote {number}")}</h1><p>{data.customer_name.clone()}</p></div><StatusLabel value=data.status.clone()/></header>
            {email_status.as_deref().map(|status| { let copy=email_outcome_copy(status); if copy.success { view! { <p class="notice" role="status">{copy.message}</p> }.into_any() } else { view! { <div class="errors" role="alert"><p>{copy.message}</p></div> }.into_any() } })}
            <section class="document-detail-summary" aria-labelledby="quote-summary"><div><p class="eyebrow">"Saved quote"</p><h2 id="quote-summary">"Quote details"</h2></div><dl><div><dt>"Issued"</dt><dd>{data.issue_date.clone()}</dd></div><div><dt>"Expires"</dt><dd>{if data.date.is_empty() { "Not set".into() } else { data.date.clone() }}</dd></div></dl></section>
            <section class="document-register quote-contents" aria-labelledby="quote-contents-title">
                <header><div><h2 id="quote-contents-title">"Contents"</h2></div><span>{format!("{} item{}", data.lines.len(), if data.lines.len() == 1 { "" } else { "s" })}</span></header>
                <dl class="document-detail-summary"><div><dt>"Customer"</dt><dd>{data.customer_name.clone()}</dd></div><div><dt>"Email"</dt><dd>{if data.customer_email.is_empty() { "Not set".into() } else { data.customer_email.clone() }}</dd></div><div><dt>"Phone"</dt><dd>{if data.customer_phone.is_empty() { "Not set".into() } else { data.customer_phone.clone() }}</dd></div><div><dt>"Billing address"</dt><dd>{if data.billing_address.is_empty() { "Not set".into() } else { data.billing_address.clone() }}</dd></div></dl>
                <div class="document-table-wrap"><table class="document-table"><thead><tr><th>"Description"</th><th>"Quantity"</th><th>"Unit"</th><th>"Unit price"</th></tr></thead><tbody>{data.lines.into_iter().map(|line| view! { <tr><th data-label="Description">{line.description}</th><td data-label="Quantity">{line.quantity}</td><td data-label="Unit">{line.unit_label}</td><td data-label="Unit price" class="document-total">{line.unit_price}</td></tr> }).collect_view()}</tbody></table></div>
                <dl class="document-detail-summary"><div><dt>"Discount"</dt><dd>{format!("${}", data.discount)}</dd></div><div><dt>"Tax rate"</dt><dd>{format!("{}%", data.tax_rate)}</dd></div></dl>
                {(!data.notes.is_empty()).then(|| view! { <div><h3>"Notes"</h3><p>{data.notes}</p></div> })}
                {(!data.terms.is_empty()).then(|| view! { <div><h3>"Terms"</h3><p>{data.terms}</p></div> })}
            </section>
            <section class="document-command-center" aria-labelledby="quote-actions"><div><p class="eyebrow">"Next action"</p><h2 id="quote-actions">{if is_accepted { "Send approved work to production" } else if is_declined { "Quote closed" } else { "Record the customer decision" }}</h2></div><div class="document-primary-actions"><a class="button secondary-button" href=format!("/quotes/{id}/edit")>"Edit quote"</a>{if is_accepted { view! { <a class="button" href=format!("/jobs/new?quote={id}")>"Create job from quote"</a><form method="post" action=format!("/quotes/{id}/create-invoice")><button type="submit">"Create invoice"</button></form> }.into_any() } else if !is_declined { view! { <form method="post" action=format!("/quotes/{id}/accepted") onsubmit="return confirm('Mark this quote accepted?')"><button type="submit">"Mark accepted"</button></form> }.into_any() } else { ().into_any() }}</div><div class="document-output-actions"><a class="button secondary-button" href=format!("/quotes/{id}/pdf") data-pdf-action="download" aria-describedby="quote-pdf-status">"Download PDF"</a><a class="button secondary-button" href=format!("/quotes/{id}/print") target="_blank" rel="noopener" data-pdf-action="print" aria-describedby="quote-pdf-status">"Print"</a>{(!is_declined).then(|| view! { <form method="post" action=format!("/quotes/{id}/declined") onsubmit="return confirm('Decline this quote?')"><button class="danger-button" type="submit">"Decline quote"</button></form> })}</div><p id="quote-pdf-status" class="pdf-action-status" role="status" aria-live="polite" hidden></p></section>
            {if data.resend_configured { view! { <details open=!email_data.errors.is_empty()><summary>"Email PDF"</summary><ErrorList errors=email_data.errors.clone()/><form class="email-form compact-email" method="post" action=format!("/quotes/{id}/email") data-unsaved-warning="Your unsent email will be lost." data-unsaved-initial=(!email_data.errors.is_empty()).then_some("true")><fieldset><legend>"Email branded quote PDF"</legend><label>"Recipient"<input required type="email" maxlength="320" name="recipient" value=email_data.recipient/></label><label>"Subject"<input required maxlength="200" name="subject" value=email_data.subject/></label><label>"Short message"<textarea maxlength="2000" name="message">{email_data.message}</textarea></label><button type="submit">"Email PDF"</button></fieldset></form></details> }.into_any() } else { view! { <p class="disabled-action" aria-disabled="true">"Email PDF unavailable — Resend is not configured."</p> }.into_any() }}
            <footer class="detail-actions"><a class="button secondary-button" href="/quotes">"Back to quotes"</a></footer>
        </article>
    }
}

#[component]
pub fn DocumentEditorPage() -> impl IntoView {
    let params = use_params_map();
    let location = use_location();
    let query = use_query_map();
    let key = move || {
        let kind = if location.pathname.read().starts_with("/quotes") {
            "quote"
        } else {
            "invoice"
        };
        (kind.to_string(), params.read().get("id"))
    };
    let data = Resource::new(key, |(kind, id)| load_document(kind, id));
    view! { <Suspense fallback=|| view! { <p role="status">"Loading document…"</p> }>{move || data.get().map(|result| match result { Ok(data) => document_form(data, query.read().get("saved").is_some(), query.read().get("email")).into_any(), Err(error) => error_view(&error.to_string()).into_any() })}</Suspense> }
}

#[derive(Clone)]
struct EditableLine {
    key: usize,
    value: LineForm,
}

#[component]
fn LineItemsEditor(initial: Vec<LineForm>, #[prop(default = false)] simple: bool) -> impl IntoView {
    let initial = if initial.is_empty() {
        vec![LineForm::default()]
    } else {
        initial
    };
    let next_key = RwSignal::new(initial.len());
    let lines = RwSignal::new(
        initial
            .into_iter()
            .enumerate()
            .map(|(key, value)| EditableLine { key, value })
            .collect::<Vec<_>>(),
    );
    view! {
        <fieldset class="document-panel line-items-panel wide">
            <legend>{if simple { "Items" } else { "03 Items & pricing" }}</legend>
            {(!simple).then(|| view! { <div class="document-panel-intro line-item-intro"><div><strong>"What are we making?"</strong><small>"Add only the products included in this estimate."</small></div><span>{move || format!("{} item{}", lines.with(Vec::len), if lines.with(Vec::len) == 1 { "" } else { "s" })}</span></div> })}
            <div class="line-head" aria-hidden="true"><span>"Description"</span><span>"Qty"</span><span>"Unit"</span><span>"Unit price"</span><span></span></div>
            <For
                each=move || lines.get()
                key=|line| line.key
                children=move |line| {
                    let key = line.key;
                    let value = line.value;
                    view! {
                        <fieldset class="line">
                            <legend>{format!("Item {}", key + 1)}</legend>
                            <span class="line-number" aria-hidden="true">{format!("{:02}", key + 1)}</span>
                            <label>"Description"<input maxlength="500" name="line_description" placeholder="Product, size, stock, finishing…" value=value.description/></label>
                            <label>"Qty"<input inputmode="decimal" name="line_quantity" placeholder="1" value=value.quantity/></label>
                            <label>"Unit"<input maxlength="50" name="line_unit" placeholder="each" value=value.unit_label/></label>
                            <label>"Unit price"<input inputmode="decimal" name="line_price" placeholder="0.00" value=value.unit_price/></label>
                            <button
                                type="button"
                                class="line-remove secondary-button"
                                aria-label=format!("Remove item {}", key + 1)
                                disabled=move || lines.with(Vec::len) == 1
                                on:click=move |_| lines.update(|items| items.retain(|item| item.key != key))
                            >"×"</button>
                        </fieldset>
                    }
                }
            />
            <button
                type="button"
                class="line-add secondary-button"
                disabled=move || { lines.with(Vec::len) >= 50 }
                on:click=move |_| {
                    let key = next_key.get_untracked();
                    next_key.set(key.saturating_add(1));
                    lines.update(|items| items.push(EditableLine { key, value: LineForm::default() }));
                }
            ><span aria-hidden="true">"+"</span>"Add item"</button>
        </fieldset>
    }
}

#[allow(clippy::needless_pass_by_value)]
fn document_form(
    data: DocumentFormData,
    saved_notice: bool,
    email_status: Option<String>,
) -> impl IntoView {
    let saved = data.id.is_some();
    let plural = format!("{}s", data.kind);
    let label = if data.kind == "quote" {
        "Quote"
    } else {
        "Invoice"
    };
    let action = data.id.as_ref().map_or_else(
        || format!("/{plural}/new"),
        |id| format!("/{plural}/{id}/edit"),
    );
    let lines = data.lines;
    let id = data.id.clone();
    let is_quote = data.kind == "quote";
    let default_email_recipient = data.customer_email.clone();
    let default_email_subject = format!("{label} {}", data.number.as_deref().unwrap_or_default());
    let email_data = data.email.unwrap_or_else(|| EmailFormData {
        recipient: default_email_recipient,
        subject: default_email_subject,
        ..EmailFormData::default()
    });
    let email_configured = data.resend_configured;
    let document_initially_dirty = !data.errors.is_empty();
    let email_initially_dirty = !email_data.errors.is_empty();
    let list_href = format!("/{plural}");
    view! {
        <article class="document-editor-shell">
            <header class="document-editor-heading"><div><a class="back-link" href=list_href.clone()>{format!("← {label}s")}</a>{(!is_quote).then(|| view! { <p class="eyebrow">"Billing document"</p> })}<h1>{format!("{label} {}", data.number.as_deref().unwrap_or("draft"))}</h1><p class="save-state">{if saved { "Saved record · edits remain local until Save." } else if is_quote { "Add the quote details, then create the quote." } else { "Unsaved draft · save once to unlock PDF and workflow actions." }}</p></div>{(!is_quote).then(|| view! { <div class="document-state"><span>{if saved { "Saved" } else { "Drafting" }}</span><small>"Payment tracking"</small></div> })}</header>
            {saved_notice.then(|| view! { <p class="notice" role="status">{if is_quote { "Quote saved. Open View quote to review or send it." } else { "Saved. PDF actions now use this current saved version." }}</p> })}
            {email_status.as_deref().map(|status| { let outcome = email_outcome_copy(status); if outcome.success { view! { <p class="notice" role="status">{outcome.message}</p> }.into_any() } else { view! { <div class="errors" role="alert"><p>{outcome.message}</p></div> }.into_any() } })}
            <ErrorList errors=data.errors.clone()/>
            <form method="post" action=action class="document-form document-workbench" class:is-quote=is_quote data-unsaved-warning="Your unsaved changes will be lost." data-unsaved-initial=document_initially_dirty.then_some("true")>
                {if is_quote { view! {
                    <fieldset class="document-panel customer-panel wide"><legend>"Quote details"</legend><label>"Customer name"<input required maxlength="200" name="customer_name" autocomplete="name" value=data.customer_name/></label><div class="document-field-pair"><label>"Email"<input type="email" maxlength="320" name="customer_email" autocomplete="email" value=data.customer_email/></label><label>"Phone"<input type="tel" maxlength="100" name="customer_phone" autocomplete="tel" value=data.customer_phone/></label></div><label>"Billing address"<textarea maxlength="2000" name="billing_address" rows="3">{data.billing_address}</textarea></label><div class="document-field-pair"><label>"Issue date"<input required type="date" name="issue_date" value=data.issue_date/></label><label>"Expiry date"<input type="date" name="expiry_or_due_date" value=data.date/></label></div><label>"Status"<select name="status"><EditorStatusOptions current=data.status kind="quote"/></select><span class="hint">"Use the saved quote to record acceptance or decline."</span></label></fieldset>
                    <LineItemsEditor initial=lines simple=true/>
                    <fieldset class="document-panel details-panel wide"><legend>"Adjustments & terms"</legend><div class="document-field-pair"><label>"Discount"<input inputmode="decimal" name="discount" value=data.discount/></label><label>"Tax rate (%)"<input inputmode="decimal" name="tax_rate" value=data.tax_rate/></label></div><div class="document-field-pair"><label>"Notes"<textarea maxlength="5000" name="notes" rows="4" placeholder="Production notes or customer context">{data.notes}</textarea></label><label>"Terms"<textarea maxlength="5000" name="terms" rows="4" placeholder="Payment, turnaround, and approval terms">{data.terms}</textarea></label></div></fieldset>
                }.into_any() } else { view! {
                    <fieldset class="document-panel customer-panel"><legend><span>"01"</span>" Customer"</legend><div class="document-panel-intro"><strong>"Who is this for?"</strong><small>"Contact details carry through to PDFs and downstream work."</small></div><label>"Customer name"<input required maxlength="200" name="customer_name" autocomplete="name" value=data.customer_name/></label><div class="document-field-pair"><label>"Email"<input type="email" maxlength="320" name="customer_email" autocomplete="email" value=data.customer_email/></label><label>"Phone"<input type="tel" maxlength="100" name="customer_phone" autocomplete="tel" value=data.customer_phone/></label></div><label>"Billing address"<textarea maxlength="2000" name="billing_address" rows="3">{data.billing_address}</textarea></label></fieldset>
                    <fieldset class="document-panel terms-panel"><legend><span>"02"</span>{format!(" {label} terms")}</legend><div class="document-panel-intro"><strong>"Set the payment window"</strong><small>"Paid status is confirmed with the action below."</small></div><div class="document-field-pair"><label>"Issue date"<input required type="date" name="issue_date" value=data.issue_date/></label><label>"Due date"<input type="date" name="expiry_or_due_date" value=data.date/></label></div><label>"Status"<select name="status"><EditorStatusOptions current=data.status kind="invoice"/></select><span class="hint">"Use the confirmed action to mark paid."</span></label></fieldset>
                    <LineItemsEditor initial=lines/>
                    <fieldset class="document-panel totals-panel"><legend><span>"04"</span>" Adjustments"</legend><div class="document-panel-intro"><strong>"Finish the numbers"</strong><small>"Exact totals are calculated when you save and render the PDF."</small></div><div class="document-field-pair"><label>"Discount"<input inputmode="decimal" name="discount" value=data.discount/></label><label>"Tax rate (%)"<input inputmode="decimal" name="tax_rate" value=data.tax_rate/></label></div></fieldset>
                    <fieldset class="document-panel details-panel"><legend><span>"05"</span>" Customer-facing details"</legend><div class="document-field-pair"><label>"Notes"<textarea maxlength="5000" name="notes" rows="4" placeholder="Production notes or customer context">{data.notes}</textarea></label><label>"Terms"<textarea maxlength="5000" name="terms" rows="4" placeholder="Payment, turnaround, and approval terms">{data.terms}</textarea></label></div></fieldset>
                }.into_any() }}
                {if is_quote { view! { <div class="document-action-bar wide"><div><strong>{if saved { "Save changes" } else { "Create quote" }}</strong></div><div class="actions"><a class="button secondary-button" href=list_href>"Cancel"</a>{id.as_ref().map(|id| view! { <a class="button secondary-button" href=format!("/quotes/{id}")>"View quote"</a> })}<button>{if saved { "Save changes" } else { "Create quote" }}</button></div></div> }.into_any() } else { view! { <div class="document-action-bar wide"><div><strong>{if saved { "Save this version" } else { "Create this invoice" }}</strong><small>{if saved { "PDF and workflow actions use the last saved version." } else { "Saving assigns an invoice number and unlocks document actions." }}</small></div><div class="actions"><button>{if saved { "Save changes" } else { "Create invoice" }}</button>{id.as_ref().map(|id| { let pdf = format!("/{plural}/{id}/pdf"); let print = format!("/{plural}/{id}/print"); view! { <a class="button secondary-button" href=pdf data-pdf-action="download" aria-describedby="pdf-action-status">"PDF"</a><a class="button secondary-button" href=print target="_blank" rel="noopener" data-pdf-action="print" aria-describedby="pdf-action-status">"Print"</a> }})}{id.as_ref().map(|_| view! { <button form="mark-paid" type="submit">"Mark paid"</button> })}</div></div>{id.as_ref().map(|_| view! { <p id="pdf-action-status" class="pdf-action-status wide" role="status" aria-live="polite" hidden></p> })} }.into_any() }}
            </form>
            {(!is_quote).then(|| id.as_ref().map(|id| if email_configured { view! {
                <ErrorList errors=email_data.errors.clone()/>
                <form method="post" action=format!("/{plural}/{id}/email") class="email-form document-email-panel" data-unsaved-warning="Your unsent email will be lost." data-unsaved-initial=email_initially_dirty.then_some("true") onsubmit="const b=this.querySelector('button');b.disabled=true;b.textContent='Sending…';">
                    <fieldset><legend>"Send saved PDF"</legend><p class="hint">"The current saved version is rendered once and attached to this message."</p><div class="document-field-pair"><label>"Recipient"<input required type="email" maxlength="320" name="recipient" value=email_data.recipient.clone()/></label><label>"Subject"<input required maxlength="200" name="subject" value=email_data.subject.clone()/></label></div><label>"Short message"<textarea maxlength="2000" name="message" rows="3">{email_data.message.clone()}</textarea></label><div class="actions"><button type="submit">"Send PDF"</button></div></fieldset>
                </form>
            }.into_any() } else { view! { <p class="disabled-action document-email-disabled" aria-disabled="true"><strong>"Email delivery is not configured."</strong><span>"PDF download and print remain available after saving."</span></p> }.into_any() }))}
            {(!is_quote).then(|| id.map(|id| view! { <form id="mark-paid" method="post" action=format!("/invoices/{id}/paid") onsubmit="return confirm('Mark this invoice paid?')"></form> }))}
        </article>
    }
}

pub(super) struct EmailOutcomeCopy {
    pub(super) success: bool,
    pub(super) message: &'static str,
}

pub(super) fn email_outcome_copy(status: &str) -> EmailOutcomeCopy {
    match status {
        "sent" => EmailOutcomeCopy {
            success: true,
            message: "Email sent. Resend confirmed acceptance and the saved record now includes the provider message ID.",
        },
        "unknown" => EmailOutcomeCopy {
            success: false,
            message: "Delivery unknown. The provider outcome or local confirmation could not be completed. Your saved document is safe. Check Resend before deciding whether to retry.",
        },
        "invalid" => EmailOutcomeCopy {
            success: false,
            message: "The email details were invalid. Check the recipient, subject, and message limits, then retry. Your saved document is safe.",
        },
        "not-configured" => EmailOutcomeCopy {
            success: false,
            message: "Resend is not configured. Add RESEND_API_KEY and RESEND_FROM, restart the app, then retry. Your saved document is safe.",
        },
        "pdf" => EmailOutcomeCopy {
            success: false,
            message: "PDF generation failed. Check the configured logo and saved document values, then retry. Resend was not contacted and your saved document is safe.",
        },
        "too-large" => EmailOutcomeCopy {
            success: false,
            message: "The generated PDF exceeds the 10 MiB email attachment limit. Resend was not contacted. Your saved document is safe; download the PDF and send it manually.",
        },
        "network" => EmailOutcomeCopy {
            success: false,
            message: "The email provider could not be reached. Your saved document is safe; retry when the network is available.",
        },
        "rejected" => EmailOutcomeCopy {
            success: false,
            message: "Resend rejected the email. Your saved document is safe; check the sender and recipient details before retrying.",
        },
        "confirmation" => EmailOutcomeCopy {
            success: false,
            message: "Resend returned an invalid confirmation, so delivery is unclear. Your saved document is safe; check Resend before deciding whether to retry.",
        },
        crate::read_models::DOCUMENT_LOAD_FAILURE_OUTCOME => EmailOutcomeCopy {
            success: false,
            message: "The saved document or business settings could not be loaded, so no email was sent. Your saved records are safe; retry after checking the database.",
        },
        _ => EmailOutcomeCopy {
            success: false,
            message: "The email request could not be completed. Your saved document is safe; review the details and retry.",
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{email_outcome_copy, updated_date};

    #[test]
    fn register_dates_are_compact_and_preserve_unknown_values() {
        assert_eq!(updated_date("2026-09-02T17:01:16.492Z"), "Sep 2, 2026");
        assert_eq!(updated_date("legacy timestamp"), "legacy timestamp");
    }

    #[test]
    fn email_outcomes_are_distinct_safe_and_actionable() {
        let statuses = [
            "unknown",
            "invalid",
            "not-configured",
            "pdf",
            "too-large",
            "network",
            "rejected",
            "confirmation",
            crate::read_models::DOCUMENT_LOAD_FAILURE_OUTCOME,
        ];
        let messages = statuses
            .into_iter()
            .map(|status| {
                let copy = email_outcome_copy(status);
                assert!(!copy.success);
                assert!(copy.message.contains("safe"), "unsafe copy for {status}");
                copy.message
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(messages.len(), statuses.len());
        assert!(
            email_outcome_copy("unknown")
                .message
                .contains("Delivery unknown")
        );
        assert!(
            email_outcome_copy("pdf")
                .message
                .contains("PDF generation failed")
        );
        assert!(email_outcome_copy("too-large").message.contains("10 MiB"));
        assert!(
            email_outcome_copy("network")
                .message
                .contains("could not be reached")
        );
        assert!(email_outcome_copy("rejected").message.contains("rejected"));
        assert!(
            email_outcome_copy("confirmation")
                .message
                .contains("invalid confirmation")
        );
        assert!(
            email_outcome_copy(crate::read_models::DOCUMENT_LOAD_FAILURE_OUTCOME)
                .message
                .contains("database")
        );
        assert!(email_outcome_copy("sent").success);
    }
}
