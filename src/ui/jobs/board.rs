use leptos::prelude::*;

use super::{ARCHIVE_PAGE_SIZE, ArchiveData, ArchiveRequest, JobView};
use crate::product_types::product_mark_key;

pub(super) fn archive_view(data: ArchiveData, filters: ArchiveRequest) -> impl IntoView {
    let ArchiveRequest {
        search,
        invoice,
        pickup,
        completed_from,
        completed_through,
        category,
        tag_id,
        ..
    } = filters;
    let page_count = data.total.div_ceil(ARCHIVE_PAGE_SIZE).max(1);
    let previous = data.page.saturating_sub(1);
    let next = data.page.saturating_add(1);
    let has_previous = data.page > 1;
    let has_next = data.page < page_count;
    view! {
        <section class="archive-page">
            <header class="archive-page-heading">
                <div><a class="back-link" href="/">"← Back to schedule"</a><h1>"Archives"</h1><p>"Older completed jobs, after the ten most recent shown on the schedule."</p></div>
                <span class="result-count">{format!("{} jobs", data.total)}</span>
            </header>
            <form class="archive-search" method="get" role="search">
                <label>"Search archives"<input type="search" name="q" value=search.clone() placeholder="Job or customer"/></label>
                <details class="filter-disclosure" open=!invoice.is_empty() || !pickup.is_empty() || !completed_from.is_empty() || !completed_through.is_empty() || !category.is_empty() || !tag_id.is_empty()>
                    <summary>"Archive filters"</summary>
                    <div class="archive-filter-grid">
                        <label>"Completed from"<input type="date" name="completed_from" value=completed_from.clone()/></label>
                        <label>"Completed through"<input type="date" name="completed_through" value=completed_through.clone()/></label>
                        <label>"Category"<select name="category"><option value="" selected=category.is_empty()>"Any category"</option>{data.categories.into_iter().map(|value| { let selected = value == category; view! { <option value=value.clone() selected=selected>{value.clone()}</option> }}).collect_view()}</select></label>
                        <label>"Invoice"<select name="invoice">
                            <option value="" selected=invoice.is_empty()>"Any invoice"</option>
                            <option value="paid" selected=invoice == "paid">"Paid"</option>
                            <option value="unpaid" selected=invoice == "unpaid">"Open"</option>
                            <option value="none" selected=invoice == "none">"No invoice"</option>
                        </select></label>
                        <label>"Pickup"<select name="pickup">
                            <option value="" selected=pickup.is_empty()>"Any pickup"</option>
                            <option value="picked-up" selected=pickup == "picked-up">"Picked up"</option>
                            <option value="awaiting" selected=pickup == "awaiting">"Awaiting pickup"</option>
                        </select></label>
                    </div>
                </details>
                <button>"Search"</button>
                <a href="/archives">"Clear"</a>
            </form>
            {if data.jobs.is_empty() {
                view! { <section class="empty"><h2>"No archived jobs found"</h2><p>"Try a broader search or clear the filters."</p></section> }.into_any()
            } else {
                view! { <div class="archive-list">{data.jobs.into_iter().map(archive_ticket).collect_view()}</div> }.into_any()
            }}
            <nav class="pagination" aria-label="Archive pages">
                <form method="get">
                    <ArchiveQueryFields search=search.clone() invoice=invoice.clone() pickup=pickup.clone() completed_from=completed_from.clone() completed_through=completed_through.clone() category=category.clone() tag_id=tag_id.clone()/>
                    <button class="secondary-button" name="page" value=previous disabled=!has_previous>"Previous"</button>
                </form>
                <span aria-live="polite">{format!("Page {} of {page_count}", data.page)}</span>
                <form method="get">
                    <ArchiveQueryFields search=search invoice=invoice pickup=pickup completed_from=completed_from completed_through=completed_through category=category tag_id=tag_id/>
                    <button class="secondary-button" name="page" value=next disabled=!has_next>"Next"</button>
                </form>
            </nav>
        </section>
    }
}

fn archive_ticket(job: JobView) -> impl IntoView {
    let detail = format!("/jobs/{}", job.id);
    let detail_title = detail.clone();
    let product_class = format!(
        "product-mark product-mark-{}",
        product_mark_key(&job.category)
    );
    view! {
        <article class="job-ticket state-complete">
            <h3><a href=detail_title rel="external" data-detail-link>{job.title}</a></h3>
            <p class=product_class><span class="visually-hidden">"Product type: "</span>{job.category}</p>
            <p class="ticket-customer">{job.customer_name}</p>
            <div class="ticket-signals"><span class="pickup-signal">{if job.picked_up { "Picked up" } else { "Awaiting pickup" }}</span><span class="due-state due-archived">{job.archive_due_label}</span></div>
            <div class="ticket-actions"><a class="ticket-view" href=detail rel="external" data-detail-link>"View archived job"</a></div>
        </article>
    }
}

#[component]
fn ArchiveQueryFields(
    search: String,
    invoice: String,
    pickup: String,
    completed_from: String,
    completed_through: String,
    category: String,
    tag_id: String,
) -> impl IntoView {
    view! {
        {(!search.is_empty()).then(|| view! { <input type="hidden" name="q" value=search/> })}
        {(!invoice.is_empty()).then(|| view! { <input type="hidden" name="invoice" value=invoice/> })}
        {(!pickup.is_empty()).then(|| view! { <input type="hidden" name="pickup" value=pickup/> })}
        {(!completed_from.is_empty()).then(|| view! { <input type="hidden" name="completed_from" value=completed_from/> })}
        {(!completed_through.is_empty()).then(|| view! { <input type="hidden" name="completed_through" value=completed_through/> })}
        {(!category.is_empty()).then(|| view! { <input type="hidden" name="category" value=category/> })}
        {(!tag_id.is_empty()).then(|| view! { <input type="hidden" name="tag_id" value=tag_id/> })}
    }
}
