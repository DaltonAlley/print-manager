#[cfg(feature = "ssr")]
use super::shared::ValidationData;
use super::shared::{ErrorList, error_view};
pub use crate::read_models::SettingsData;
use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

#[server]
pub async fn load_settings() -> Result<SettingsData, ServerFnError> {
    use crate::app_services::AppServices;
    if let Some(ValidationData::Settings(data)) = use_context::<ValidationData>() {
        return Ok(data);
    }
    let services = expect_context::<AppServices>();
    Ok(services.settings().await?)
}

#[component]
pub fn SettingsPage() -> impl IntoView {
    let query = use_query_map();
    let data = Resource::new(|| (), |()| load_settings());
    view! { <Suspense fallback=|| view! { <p role="status">"Loading settings…"</p> }>{move || data.get().map(|result| match result { Ok(data) => settings_form(data, query.read().get("saved").is_some()).into_any(), Err(error) => error_view(&error.to_string()).into_any() })}</Suspense> }
}

fn settings_form(data: SettingsData, saved: bool) -> impl IntoView {
    let has_errors = !data.errors.is_empty();
    let has_logo = data.has_logo;
    view! {
        <h1>"Settings"</h1><p class="save-state">{if saved { "Settings saved — further edits are unsaved until you press Save settings." } else if has_errors { "Unsaved settings — fix the errors and press Save settings." } else { "Saved settings — edits are unsaved until you press Save settings." }}</p>
        {saved.then(|| view! { <p class="notice" role="status">"Settings saved."</p> })}<ErrorList errors=data.errors.clone()/>
        <form method="post" class="editor" data-unsaved-warning="Your unsaved changes will be lost." data-unsaved-initial=has_errors.then_some("true")><label>"Business name"<input required maxlength="200" name="business_name" value=data.business_name/></label><label>"Address"<textarea maxlength="2000" name="address">{data.address}</textarea></label><label>"Phone"<input maxlength="100" name="phone" value=data.phone/></label><label>"Email"<input type="email" maxlength="320" name="email" value=data.email/></label><label>"Default tax rate (%)"<input inputmode="decimal" name="default_tax_rate" value=data.default_tax_rate/></label><label>"Default turnaround (days)"<input required type="number" min="1" max="365" name="default_turnaround_days" value=data.default_turnaround_days/><span class="hint">"Sets the initial due date for every new print job. A job's due date can still be changed."</span></label><label>"Quote prefix"<input required maxlength="20" name="quote_prefix" value=data.quote_prefix/></label><label>"Invoice prefix"<input required maxlength="20" name="invoice_prefix" value=data.invoice_prefix/></label><p class="wide config-state"><strong>"Resend: "</strong>{if data.resend_configured { "Configured" } else { "Not configured" }}</p><div class="actions wide"><button>"Save settings"</button></div></form>
        <section class="settings-section" aria-labelledby="managed-logo"><h2 id="managed-logo">"Managed logo"</h2><p class="hint">"Upload a PNG or JPEG up to 2 MiB and 2000 × 2000 pixels. The current logo changes only after validation succeeds."</p>{has_logo.then(|| view! { <img class="logo-preview" src="/settings/logo/current" alt="Current business logo"/> })}<p class="config-state" role="status">{if has_logo { "A managed logo is configured and will brand saved quote and invoice PDFs." } else { "No managed logo is configured." }}</p><form class="upload-form" method="post" action="/settings/logo" enctype="multipart/form-data"><label>"Choose replacement logo"<input required type="file" name="file" accept="image/png,image/jpeg"/></label><button type="submit">"Upload logo"</button></form></section>
    }
}
