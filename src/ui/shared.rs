use leptos::prelude::*;

#[cfg(feature = "ssr")]
use super::{documents::DocumentFormData, jobs::JobFormData, settings::SettingsData};

#[cfg(feature = "ssr")]
#[derive(Clone)]
pub(crate) enum ValidationData {
    Job(JobFormData),
    Document(Box<DocumentFormData>),
    Settings(SettingsData),
}

#[component]
#[allow(clippy::needless_pass_by_value)]
pub(super) fn EditorStatusOptions(current: String, kind: &'static str) -> impl IntoView {
    let (terminal, mut values) = match kind {
        "job" => (
            "",
            vec![
                ("design", "Designing"),
                ("waiting", "Waiting for customer"),
                ("print", "Ready to print"),
                ("finish", "Ready to finish"),
                ("complete", "Completed"),
            ],
        ),
        "quote" => (
            "declined",
            if current == "accepted" {
                vec![
                    ("draft", "Draft"),
                    ("sent", "Sent"),
                    ("accepted", "Accepted"),
                ]
            } else {
                vec![("draft", "Draft"), ("sent", "Sent")]
            },
        ),
        _ => ("paid", vec![("draft", "Draft"), ("sent", "Sent")]),
    };
    if current == terminal {
        let label = match terminal {
            "" => "",
            "declined" => "Declined",
            _ => "Paid",
        };
        values.push((terminal, label));
    }
    values.into_iter().map(|(value, label)| view! { <option value=value selected=current == value>{label}</option> }).collect_view()
}

#[component]
pub(super) fn ErrorList(errors: Vec<String>) -> impl IntoView {
    (!errors.is_empty()).then(|| view! { <div class="errors" role="alert"><h2>"Please fix the following"</h2><ul>{errors.into_iter().map(|error| view! { <li>{error}</li> }).collect_view()}</ul><p>"Your entered values are still in the form."</p></div> })
}

#[component]
#[allow(clippy::needless_pass_by_value)]
pub(super) fn StatusLabel(value: String) -> impl IntoView {
    let status_class = format!("status status-{}", value.replace('_', "-"));
    let label = match value.as_str() {
        "design" => "Designing",
        "waiting" => "Waiting for customer",
        "print" => "Ready to print",
        "finish" => "Ready to finish",
        "complete" => "Completed",
        "draft" => "Draft",
        "sent" => "Sent",
        "accepted" => "Accepted",
        "paid" => "Paid",
        "declined" => "Declined",
        _ => "Unknown",
    };
    view! { <span class=status_class><span class="status-dot" aria-hidden="true"></span>{label}</span> }
}

pub(super) fn money(cents: i64) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}
pub(super) fn error_view(message: &str) -> impl IntoView {
    view! { <h1>"Something went wrong"</h1><p role="status">{format!("{message}. Your saved records are safe.")}</p> }
}
#[component]
pub(crate) fn NotFoundPage() -> impl IntoView {
    view! { <h1>"Not found"</h1><p>"That page does not exist."</p> }
}
