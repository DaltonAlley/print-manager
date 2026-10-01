use std::collections::HashMap;

use leptos::{ev, prelude::*};

use super::{
    BoardCommandRequest, JobsData, ScheduleJobRequest, ScheduleJobResult, WorkflowFilters,
    billing_label, card_state_class, execute_board_command, load_jobs, schedule_job,
};
use crate::read_models::{
    BoardCommand, BoardCommandLane, BoardCommandOutcome, BoardCommandPlacement, BoardCommandResult,
    JobView, TimelineDayView,
};

type StartAction = Action<BoardCommandRequest, ()>;
type ScheduleAction = Action<ScheduleJobRequest, ()>;
type RecoveryAction = Action<(), ()>;

#[derive(Clone, Copy)]
struct ScheduleInteraction {
    start_action: StartAction,
    schedule_action: ScheduleAction,
    filters: StoredValue<WorkflowFilters>,
    refresh_required: RwSignal<bool>,
    drag: RwSignal<ScheduleDragState>,
    announcement: RwSignal<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ScheduleDragState {
    job: Option<DraggedJob>,
    target_date: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DraggedJob {
    id: String,
    due_date: String,
}

impl ScheduleDragState {
    fn start(&mut self, id: String, due_date: String) {
        self.job = Some(DraggedJob { id, due_date });
        self.target_date = None;
    }

    fn hover(&mut self, due_date: &str) -> bool {
        let accepts = self
            .job
            .as_ref()
            .is_some_and(|job| job.due_date != due_date);
        self.target_date = accepts.then(|| due_date.to_owned());
        accepts
    }

    fn finish(&mut self) {
        *self = Self::default();
    }

    fn is_dragged(&self, id: &str) -> bool {
        self.job.as_ref().is_some_and(|job| job.id == id)
    }

    fn is_target(&self, due_date: &str) -> bool {
        self.target_date.as_deref() == Some(due_date)
    }
}

impl ScheduleInteraction {
    fn commands_disabled(self) -> bool {
        self.start_action.pending().get()
            || self.schedule_action.pending().get()
            || self.refresh_required.get()
    }

    fn commands_disabled_untracked(self) -> bool {
        self.start_action.pending().get_untracked()
            || self.schedule_action.pending().get_untracked()
            || self.refresh_required.get_untracked()
    }

    fn schedule(self, job_id: String, due_date: String) {
        if self.commands_disabled_untracked() {
            return;
        }
        let filters = self.filters.get_value();
        self.schedule_action.dispatch(ScheduleJobRequest {
            job_id,
            due_date,
            search: filters.search,
            category: filters.category,
            status: filters.stage,
            material: filters.material,
            billing: filters.billing,
        });
    }

    fn move_job(self, job_id: String, source: BoardCommandLane, destination: BoardCommandLane) {
        if self.commands_disabled_untracked() || source == destination {
            return;
        }
        let filters = self.filters.get_value();
        self.start_action.dispatch(BoardCommandRequest {
            command: BoardCommand {
                source,
                destination,
                card_id: job_id,
                placement: BoardCommandPlacement::Last,
            },
            search: filters.search,
            category: filters.category,
            status: filters.stage,
            material: filters.material,
            billing: filters.billing,
        });
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn jobs_view(
    data: JobsData,
    filters: (String, String, String, String, String),
    saved: bool,
    outcome_notice: Option<&'static str>,
) -> impl IntoView {
    let (search, category, status, material, billing) = filters;
    let board_data = RwSignal::new(data);
    // Board cards contain event handlers that capture their job identity. Force a
    // fresh keyed board after each authoritative refresh so a card moved between
    // columns can never retain another position's handlers.
    let board_revision = RwSignal::new(0_u64);
    let notice = RwSignal::new(
        outcome_notice
            .or(saved.then_some("Job saved."))
            .map(str::to_owned),
    );
    let error = RwSignal::<Option<String>>::new(None);
    let refresh_required = RwSignal::new(false);
    let stored_filters = StoredValue::new(WorkflowFilters {
        search: search.clone(),
        category: category.clone(),
        stage: status.clone(),
        material: material.clone(),
        billing: billing.clone(),
    });

    let start_action = Action::new(move |request: &BoardCommandRequest| {
        let request = request.clone();
        let job_id = request.command.card_id.clone();
        async move {
            match execute_board_command(request).await {
                Ok(BoardCommandResult::Refreshed { data, outcome }) => {
                    let message = workflow_notice(&outcome);
                    let focus_id = match &outcome {
                        BoardCommandOutcome::Started(id) => id,
                        _ => &job_id,
                    };
                    board_data.set(*data);
                    board_revision.update(|revision| *revision += 1);
                    notice.set(Some(message.into()));
                    error.set(None);
                    schedule_focus(focus_id);
                }
                Ok(BoardCommandResult::CommittedRefreshFailed { outcome }) => {
                    refresh_required.set(true);
                    notice.set(None);
                    error.set(Some(format!(
                        "{} Refresh the schedule to see the saved change.",
                        workflow_notice(&outcome)
                    )));
                }
                Err(_) => {
                    refresh_required.set(true);
                    notice.set(None);
                    error.set(Some(
                        "Could not confirm the job update. Refresh the schedule and check its current status before trying again.".into(),
                    ));
                }
            }
        }
    });

    let schedule_action = Action::new(move |request: &ScheduleJobRequest| {
        let request = request.clone();
        async move {
            match schedule_job(request).await {
                Ok(ScheduleJobResult::Refreshed { data, outcome }) => {
                    board_data.set(*data);
                    board_revision.update(|revision| *revision += 1);
                    notice.set(Some(schedule_notice(&outcome.due_date)));
                    error.set(None);
                    schedule_focus(&outcome.job_id);
                }
                Ok(ScheduleJobResult::CommittedRefreshFailed { .. }) => {
                    refresh_required.set(true);
                    notice.set(None);
                    error.set(Some(
                        "The due date was saved, but the schedule could not refresh.".into(),
                    ));
                }
                Err(_) => {
                    refresh_required.set(true);
                    notice.set(None);
                    error.set(Some(
                        "Could not confirm the due date update. Refresh the schedule and check its current date before trying again.".into(),
                    ));
                }
            }
        }
    });

    let recovery = Action::new(move |(): &()| {
        let filters = stored_filters.get_value();
        async move {
            match load_jobs(
                filters.search,
                filters.category,
                filters.stage,
                filters.material,
                filters.billing,
            )
            .await
            {
                Ok(data) => {
                    board_data.set(data);
                    board_revision.update(|revision| *revision += 1);
                    notice.set(Some("Schedule refreshed. Check the job before making another change.".into()));
                    error.set(None);
                    refresh_required.set(false);
                }
                Err(_) => error.set(Some("Could not refresh the schedule. Check your connection and try refreshing again.".into())),
            }
        }
    });

    let interaction = ScheduleInteraction {
        start_action,
        schedule_action,
        filters: stored_filters,
        refresh_required,
        drag: RwSignal::new(ScheduleDragState::default()),
        announcement: RwSignal::new(String::new()),
    };

    schedule_page(
        board_data,
        board_revision,
        search,
        category,
        status,
        material,
        billing,
        notice,
        error,
        refresh_required,
        interaction,
        recovery,
    )
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn schedule_page(
    board_data: RwSignal<JobsData>,
    board_revision: RwSignal<u64>,
    search: String,
    category: String,
    status: String,
    material: String,
    billing: String,
    notice: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    refresh_required: RwSignal<bool>,
    interaction: ScheduleInteraction,
    recovery: RecoveryAction,
) -> impl IntoView {
    let has_filters = !search.is_empty()
        || !category.is_empty()
        || !status.is_empty()
        || !material.is_empty()
        || !billing.is_empty();
    let categories = board_data.with_untracked(|data| data.categories.clone());
    let materials = board_data.with_untracked(|data| data.materials.clone());
    let filter_summary = [
        (!search.is_empty()).then(|| format!("Search: {search}")),
        (!status.is_empty()).then(|| format!("State: {}", card_status_label(&status))),
        (!category.is_empty()).then(|| format!("Product: {category}")),
        (!material.is_empty()).then(|| format!("Material: {material}")),
        (!billing.is_empty()).then(|| {
            format!(
                "Billing: {}",
                match billing.as_str() {
                    "paid" => "Paid",
                    "unpaid" => "Unpaid",
                    "none" => "No invoice",
                    _ => "Any billing",
                }
            )
        }),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    view! {
        <section class="workflow-page" data-production-board>
            <h1 class="visually-hidden">"Production schedule"</h1>
            <div class="board-toolbar">
                <form class="board-search" method="get" role="search">
                    <label class="visually-hidden" for="board-search">"Search jobs, customers, and materials"</label>
                    <input id="board-search" type="search" name="q" value=search.clone() placeholder="Search jobs, customers, or materials"/>
                    {(!category.is_empty()).then(|| view! { <input type="hidden" name="category" value=category.clone()/> })}
                    {(!status.is_empty()).then(|| view! { <input type="hidden" name="status" value=status.clone()/> })}
                    {(!material.is_empty()).then(|| view! { <input type="hidden" name="material" value=material.clone()/> })}
                    {(!billing.is_empty()).then(|| view! { <input type="hidden" name="billing" value=billing.clone()/> })}
                    <button class="search-button">"Search"</button>
                </form>
                <details class="filter-disclosure">
                    <summary>"Filters"{has_filters.then_some(" · active")}</summary>
                    <DescriptorFilters search=search.clone() category=category.clone() status=status.clone() material=material.clone() billing=billing.clone() categories=categories materials=materials/>
                </details>
                <a class="button board-new-job" href="/jobs/new"><span aria-hidden="true">"+"</span>"New job"</a>
            </div>
            {has_filters.then(|| view! { <div class="schedule-filter-summary"><p>{filter_summary}</p><a href="/">"Clear filters"</a></div> })}
            <div class="schedule-feedback" aria-live="polite" role="status">{move || {
                if recovery.pending().get() { Some("Refreshing schedule…".to_owned()) }
                else if interaction.start_action.pending().get() { Some("Updating job status…".to_owned()) }
                else if interaction.schedule_action.pending().get() { Some("Saving due date…".to_owned()) }
                else { notice.get() }
            }}</div>
            {move || error.get().map(|message| view! { <div class="errors schedule-error" role="alert"><p>{message}</p>{refresh_required.get().then(|| view! { <button type="button" disabled=move || recovery.pending().get() on:click=move |_| { recovery.dispatch(()); }>{move || if recovery.pending().get() { "Refreshing…" } else { "Refresh schedule" }}</button> })}</div> })}
            <For
                each=move || vec![(board_revision.get(), board_data.get())]
                key=|(revision, _)| *revision
                children=move |(_, data)| schedule_board(data, has_filters, interaction)
            />
        </section>
    }
}

#[component]
#[allow(clippy::too_many_arguments)]
fn DescriptorFilters(
    search: String,
    category: String,
    status: String,
    material: String,
    billing: String,
    categories: Vec<String>,
    materials: Vec<String>,
) -> AnyView {
    view! {
        <form class="compact-filters" method="get">
            {(!search.is_empty()).then(|| view! { <input type="hidden" name="q" value=search/> })}
            <label>"Job state"<select name="status"><option value="" selected=status.is_empty()>"Any state"</option><option value="design" selected=status == "design">"Designing"</option><option value="waiting" selected=status == "waiting">"Waiting for customer"</option><option value="print" selected=status == "print">"Ready to print"</option><option value="finish" selected=status == "finish">"Ready to finish"</option><option value="complete" selected=status == "complete">"Completed"</option></select></label>
            <label>"Product"<select name="category"><option value="">"Any product"</option>{categories.into_iter().map(|value| { let selected = value == category; view! { <option value=value.clone() selected=selected>{value.clone()}</option> }}).collect_view()}</select></label>
            <label>"Material"<select name="material"><option value="" selected=material.is_empty()>"Any material"</option>{materials.into_iter().map(|value| { let selected = value == material; view! { <option value=value.clone() selected=selected>{value.clone()}</option> }}).collect_view()}</select></label>
            <label>"Billing"<select name="billing"><option value="" selected=billing.is_empty()>"Any billing"</option><option value="paid" selected=billing == "paid">"Paid"</option><option value="unpaid" selected=billing == "unpaid">"Unpaid"</option><option value="none" selected=billing == "none">"No invoice"</option></select></label>
            <div class="filter-actions"><button>"Apply"</button><a href="/">"Clear"</a></div>
        </form>
    }
    .into_any()
}

fn workflow_notice(outcome: &BoardCommandOutcome) -> &'static str {
    match outcome {
        BoardCommandOutcome::Started(_) => "Quote started as a scheduled job.",
        BoardCommandOutcome::Moved(BoardCommandLane::Design) => "Job moved to Designing.",
        BoardCommandOutcome::Moved(BoardCommandLane::Waiting) => {
            "Job moved to Waiting for customer."
        }
        BoardCommandOutcome::Moved(BoardCommandLane::Print) => "Job moved to Ready to print.",
        BoardCommandOutcome::Moved(BoardCommandLane::Finish) => "Job moved to Ready to finish.",
        BoardCommandOutcome::Moved(BoardCommandLane::Complete) => "Job completed.",
        BoardCommandOutcome::Moved(BoardCommandLane::Quote) => "Job moved to Quotes.",
        BoardCommandOutcome::Sorted => "Job order updated.",
    }
}

fn schedule_notice(due_date: &str) -> String {
    if due_date.is_empty() {
        "Job moved to No due date.".into()
    } else {
        format!("Job due date changed to {due_date}.")
    }
}

fn schedule_board(data: JobsData, has_filters: bool, interaction: ScheduleInteraction) -> AnyView {
    let mut columns = schedule_columns(data.jobs, &data.timeline_days);
    let active_jobs = columns
        .iter()
        .map(|column| column.jobs.len())
        .sum::<usize>();
    let overdue_jobs = columns
        .iter()
        .flat_map(|column| column.jobs.iter())
        .filter(|job| job.due_class == "due-overdue")
        .count();
    let (health_label, health_message, health_class) = if active_jobs == 0 {
        if has_filters {
            (
                "Nothing to review".to_owned(),
                "Try another view or clear the filters.".to_owned(),
                "is-clear",
            )
        } else {
            (
                "Queue clear".to_owned(),
                "No active jobs are waiting.".to_owned(),
                "is-clear",
            )
        }
    } else if overdue_jobs > 0 {
        (
            format!(
                "{} {} need attention",
                overdue_jobs,
                if overdue_jobs == 1 { "job" } else { "jobs" }
            ),
            "Start with the oldest work first.".to_owned(),
            "is-attention",
        )
    } else if has_filters {
        (
            "This view is on track".to_owned(),
            "No jobs in this view are overdue.".to_owned(),
            "is-on-track",
        )
    } else {
        (
            "On track".to_owned(),
            "No active jobs are overdue.".to_owned(),
            "is-on-track",
        )
    };
    let health_aria = format!("{health_label}. {health_message}");
    let active_job_label = if active_jobs == 1 {
        "active job"
    } else {
        "active jobs"
    };
    let completed = columns
        .iter()
        .flat_map(|column| column.completed.clone())
        .collect::<Vec<_>>();
    let Some(later_or_undated) = columns.pop() else {
        return view! { <p class="errors" role="alert">"The schedule could not be displayed."</p> }
            .into_any();
    };
    debug_assert_eq!(later_or_undated.class, "is-unscheduled");
    view! {
        <section class="due-schedule" data-leptos-view="jobs" aria-label="Print jobs sorted by due date">
            <header class="schedule-heading">
                <div class="schedule-heading-copy">
                    <p class="eyebrow">"Production queue"</p>
                    <h2>"Your shop at a glance"</h2>
                    <p>{format!("{active_jobs} {active_job_label}, sorted by due date.")}</p>
                </div>
                <div class=format!("schedule-health {health_class}") aria-label=health_aria>
                    <span class="schedule-health-mark" aria-hidden="true">{if health_class == "is-attention" { "!" } else { "✓" }}</span>
                    <span class="schedule-health-copy"><strong>{health_label}</strong><small>{health_message}</small></span>
                </div>
            </header>
            <p id="schedule-drag-instructions" class="visually-hidden">"Move a focused job one date earlier or later with Alt+Left Arrow or Alt+Right Arrow. Use its job actions menu to choose an exact date."</p>
            <p class="visually-hidden" role="status" aria-live="polite" aria-atomic="true">{move || interaction.announcement.get()}</p>
            {if active_jobs == 0 {
                let (title, message) = if has_filters { ("Nothing matches this view", "Try a different search or clear the filters to see all scheduled work.") } else { ("The schedule is clear", "Add a job when new production work comes in.") };
                view! { <section class="schedule-empty-state"><span class="empty-state-visual" class:is-filtered=has_filters aria-hidden="true"><span class="empty-state-icon"></span></span><div class="empty-state-copy"><p class="eyebrow">{if has_filters { "Filtered schedule" } else { "Production queue" }}</p><h2>{title}</h2><p>{message}</p></div><div class="schedule-empty-actions">{has_filters.then(|| view! { <a class="button" href="/">"Clear filters"</a> })}{(!has_filters).then(|| view! { <a class="button" href="/jobs/new">"New job"</a> })}</div></section> }.into_any()
            } else {
                view! { <div class="schedule-columns"><div class="date-columns" aria-label="Dated print jobs">{columns.into_iter().map(|column| view! { <DateColumn column=column interaction=interaction/> }).collect_view()}</div><DateColumn column=later_or_undated interaction=interaction/></div> }.into_any()
            }}
            {(!completed.is_empty()).then(|| view! { <details class="completed-shelf"><summary><h2>"Recently completed"</h2><span>{format!("{} jobs", completed.len())}</span></summary><div>{completed.into_iter().map(completed_ticket).collect_view()}</div></details> })}
        </section>
    }
    .into_any()
}

#[derive(Clone)]
struct DateColumnView {
    key: String,
    eyebrow: String,
    title: String,
    class: &'static str,
    drop_date: Option<String>,
    jobs: Vec<JobView>,
    completed: Vec<JobView>,
}

fn schedule_columns(jobs: Vec<JobView>, days: &[TimelineDayView]) -> Vec<DateColumnView> {
    let today = days
        .first()
        .map(|day| day.date.as_str())
        .unwrap_or_default();
    let visible_days = days.iter().take(4).collect::<Vec<_>>();
    let mut active_by_date = HashMap::<String, Vec<JobView>>::new();
    let mut completed = Vec::new();
    for job in jobs {
        if job.lifecycle == "active" {
            active_by_date
                .entry(job.due_date.clone())
                .or_default()
                .push(job);
        } else {
            completed.push(job);
        }
    }

    let mut columns = Vec::new();
    let overdue = active_by_date
        .keys()
        .filter(|date| !date.is_empty() && date.as_str() < today)
        .cloned()
        .collect::<Vec<_>>();
    let mut today_and_overdue = take_dates(&mut active_by_date, overdue);
    today_and_overdue.extend(active_by_date.remove(today).unwrap_or_default());
    if let Some(day) = visible_days.first() {
        columns.push(DateColumnView {
            key: day.date.clone(),
            eyebrow: day.weekday.clone(),
            title: "Today & overdue".into(),
            class: "is-today",
            drop_date: Some(day.date.clone()),
            jobs: today_and_overdue,
            completed: Vec::new(),
        });
    }
    for day in visible_days.into_iter().skip(1) {
        let jobs = active_by_date.remove(&day.date).unwrap_or_default();
        columns.push(DateColumnView {
            key: day.date.clone(),
            eyebrow: day.weekday.clone(),
            title: day.label.clone(),
            class: "is-date",
            drop_date: Some(day.date.clone()),
            jobs,
            completed: Vec::new(),
        });
    }
    let remaining_dates = active_by_date
        .keys()
        .filter(|date| !date.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    let mut later_or_undated = take_dates(&mut active_by_date, remaining_dates);
    later_or_undated.extend(active_by_date.remove("").unwrap_or_default());
    columns.push(DateColumnView {
        key: "later-or-undated".into(),
        eyebrow: String::new(),
        title: "Later / Undated".into(),
        class: "is-unscheduled",
        drop_date: None,
        jobs: later_or_undated,
        completed,
    });
    columns
}

fn take_dates(map: &mut HashMap<String, Vec<JobView>>, mut dates: Vec<String>) -> Vec<JobView> {
    dates.sort();
    dates
        .into_iter()
        .flat_map(|date| map.remove(&date).unwrap_or_default())
        .collect()
}

#[component]
fn DateColumn(column: DateColumnView, interaction: ScheduleInteraction) -> impl IntoView {
    let DateColumnView {
        key,
        eyebrow,
        title,
        class,
        drop_date,
        jobs,
        ..
    } = column;
    let count = jobs.len();
    let aria_label = format!(
        "{title}: {count} {}",
        if count == 1 { "job" } else { "jobs" }
    );
    let heading_title = title.clone();
    let target_date = drop_date.clone().unwrap_or_default();
    let enter_date = target_date.clone();
    let hover_date = target_date.clone();
    let dropped_date = target_date.clone();
    let drop_target = drop_date.is_some();
    let show_due_dates = class == "is-unscheduled";
    view! {
        <section class=format!("date-column {class}") data-date-column=key aria-label=aria_label class:is-drop-target=move || drop_target && interaction.drag.with(|state| state.is_target(&target_date)) on:dragenter=move |event| { if drop_target { accept_date_drag(&event, interaction, &enter_date); } } on:dragover=move |event| { if drop_target { accept_date_drag(&event, interaction, &hover_date); } } on:drop=move |event| { if !drop_target { return; } let dragged = interaction.drag.with_untracked(|state| state.job.clone()); interaction.drag.update(ScheduleDragState::finish); if let Some(job) = dragged && job.due_date != dropped_date { event.prevent_default(); interaction.announcement.set(if dropped_date.is_empty() { "Moving job to No due date.".into() } else { format!("Moving job to {dropped_date}.") }); interaction.schedule(job.id, dropped_date.clone()); } }>
            <header class="date-column-heading"><div><h2>{(!eyebrow.is_empty()).then(|| view! { <span class="date-weekday">{eyebrow}</span><span class="date-separator" aria-hidden="true">"·"</span> })}<span class="date-label">{heading_title}</span></h2></div><strong>{count}</strong></header>
            <div class="date-column-jobs">{if jobs.is_empty() { view! { <p class="date-column-empty">{if drop_target { "Drop a job here" } else { "No jobs" }}</p> }.into_any() } else { jobs.into_iter().map(|job| schedule_ticket(job, interaction, show_due_dates)).collect_view().into_any() }}</div>
        </section>
    }
}

fn accept_date_drag(event: &ev::DragEvent, interaction: ScheduleInteraction, due_date: &str) {
    let mut accepts = false;
    interaction
        .drag
        .update(|state| accepts = state.hover(due_date));
    if accepts {
        event.prevent_default();
        if let Some(transfer) = event.data_transfer() {
            transfer.set_drop_effect("move");
        }
    }
}

fn schedule_ticket(job: JobView, interaction: ScheduleInteraction, show_due_date: bool) -> AnyView {
    // Browsers require drag data before they will dispatch a drop event.
    let id = job.id.clone();
    let detail = format!("/jobs/{id}");
    let element_id = format!("schedule-card-{id}");
    let grabbed_id = id.clone();
    let data_id = id.clone();
    let form_id = id.clone();
    let clear_id = id.clone();
    let detail_data = detail.clone();
    let detail_title = detail.clone();
    let due_value = RwSignal::new(job.due_date.clone());
    let drag_id = id.clone();
    let drag_due = job.due_date.clone();
    let keyboard_due = drag_due.clone();
    let state_id = id.clone();
    let key_id = id.clone();
    let label = format!("Job {}", job.title);
    let source_status = job.status.clone();
    let descriptor_status = job.status.clone();
    let descriptor_product = job.category.clone();
    let descriptor_material = job.material.clone();
    let descriptor_invoice_id = job.invoice_id.clone();
    let descriptor_invoice_status = job.invoice_status.clone();
    let schedule_context = show_due_date.then(|| {
        if job.due_date.is_empty() {
            (false, String::from("Not set"))
        } else {
            (
                true,
                job.due_label
                    .strip_prefix("Due ")
                    .unwrap_or(&job.due_label)
                    .to_owned(),
            )
        }
    });
    let overdue_label = (job.due_class == "due-overdue").then(|| job.due_label.clone());
    let card_class = format!(
        "job-ticket schedule-ticket uniform-ticket sortable-ticket {}",
        card_state_class(&job.status)
    );
    let is_overdue = job.due_class == "due-overdue";
    let status_value = RwSignal::new(job.status.clone());
    view! {
        <article id=element_id class=card_class class:is-long-horizon=show_due_date class:is-overdue=is_overdue class:is-dragging=move || interaction.drag.with(|state| state.is_dragged(&state_id)) tabindex="0" aria-describedby="schedule-drag-instructions" aria-grabbed=move || interaction.drag.with(|state| state.is_dragged(&grabbed_id)).to_string() draggable="true" data-job-id=data_id data-card-detail=detail_data on:dragstart=move |event| { if interaction.commands_disabled_untracked() { event.prevent_default(); return; } else if let Some(transfer) = event.data_transfer() { transfer.set_effect_allowed("move"); let _ = transfer.set_data("text/plain", &drag_id); } interaction.drag.update(|state| state.start(drag_id.clone(), drag_due.clone())); } on:dragend=move |_| interaction.drag.update(ScheduleDragState::finish) on:keydown=move |event| keyboard_schedule(&event, key_id.clone(), &keyboard_due, &label, interaction)>
            <div class="ticket-heading"><span class="drag-grip" aria-hidden="true">"⠿"</span><h3><a href=detail_title rel="external" data-detail-link title=job.title.clone()>{job.title.clone()}</a></h3></div>
            <div class="ticket-subline"><p class="ticket-customer">{job.customer_name}</p>{overdue_label.map(|label| view! { <span class="ticket-due overdue-tag" title=label>"Overdue"</span> })}{schedule_context.map(|(has_due_date, value)| view! { <span class="ticket-due" class:is-undated=!has_due_date>{if has_due_date { format!("Due {value}") } else { String::from("No due date") }}</span> })}</div>
            <JobDescriptors status=descriptor_status product=descriptor_product material=descriptor_material invoice_id=descriptor_invoice_id invoice_status=descriptor_invoice_status/>
            <details class="ticket-menu"><summary aria-label="Edit job status and due date">"Edit"</summary><div class="ticket-menu-popover"><form class="ticket-menu-form" data-preserves-drafts on:submit=move |event| { event.prevent_default(); if let (Some(source), Some(destination)) = (production_lane(&source_status), production_lane(&status_value.get_untracked())) { interaction.move_job(id.clone(), source, destination); } }><label>"Status"<select name="status" prop:value=move || status_value.get() on:change=move |event| status_value.set(event_target_value(&event))><option value="design">"Designing"</option><option value="waiting">"Waiting for customer"</option><option value="print">"Ready to print"</option><option value="finish">"Ready to finish"</option><option value="complete">"Completed"</option></select></label><button type="submit" disabled=move || interaction.commands_disabled()>"Update status"</button></form><form class="ticket-menu-form" data-preserves-drafts on:submit=move |event| { event.prevent_default(); interaction.schedule(form_id.clone(), due_value.get_untracked()); }><label>"Due date"<input type="date" name="due_date" value=move || due_value.get() on:input=move |event| due_value.set(event_target_value(&event))/></label><div class="ticket-menu-form-actions"><button type="submit" disabled=move || interaction.commands_disabled()>"Set date"</button><button type="button" class="secondary-button" disabled=move || interaction.commands_disabled() on:click=move |_| interaction.schedule(clear_id.clone(), String::new())>"Clear"</button></div></form></div></details>
        </article>
    }.into_any()
}

#[component]
fn JobDescriptors(
    status: String,
    product: String,
    material: String,
    invoice_id: Option<String>,
    invoice_status: Option<String>,
) -> AnyView {
    let status_class = format!("status status-{}", status.replace('_', "-"));
    let status_label = card_status_label(&status);
    let product = descriptor_value(&product);
    let material = descriptor_value(&material);
    let billing = billing_descriptor(invoice_status.as_deref(), invoice_id.as_deref());
    let billing_class = billing.map(|billing| match billing {
        "Paid" => "is-paid",
        _ => "is-unpaid",
    });

    view! {
        <dl class="descriptor-list" aria-label="Job details">
            <div class="descriptor-item descriptor-state"><dt>"State"</dt><dd><span class=status_class><span class="status-dot" aria-hidden="true"></span><span class="status-text">{status_label}</span></span></dd></div>
            {billing.zip(billing_class).map(|(billing, class)| view! { <div class=format!("descriptor-item descriptor-billing {class}")><dt>"Billing"</dt><dd>{billing}</dd></div> })}
            {product.map(|product| view! { <div class="descriptor-item descriptor-product"><dt>"Product"</dt><dd title=product.clone()>{product.clone()}</dd></div> })}
            {material.map(|material| view! { <div class="descriptor-item descriptor-material"><dt>"Material"</dt><dd title=material.clone()>{material.clone()}</dd></div> })}
        </dl>
    }
    .into_any()
}

fn descriptor_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn billing_descriptor(
    invoice_status: Option<&str>,
    invoice_id: Option<&str>,
) -> Option<&'static str> {
    invoice_id
        .is_some_and(|invoice_id| !invoice_id.trim().is_empty())
        .then(|| billing_label(invoice_status, true))
}

fn card_status_label(value: &str) -> &'static str {
    match value {
        "design" => "Designing",
        "waiting" => "Waiting",
        "print" => "Print ready",
        "finish" => "Finish ready",
        "complete" => "Completed",
        _ => "Unknown",
    }
}

fn production_lane(value: &str) -> Option<BoardCommandLane> {
    match value {
        "design" => Some(BoardCommandLane::Design),
        "waiting" => Some(BoardCommandLane::Waiting),
        "print" => Some(BoardCommandLane::Print),
        "finish" => Some(BoardCommandLane::Finish),
        "complete" => Some(BoardCommandLane::Complete),
        _ => None,
    }
}

fn keyboard_schedule(
    event: &ev::KeyboardEvent,
    id: String,
    due_date: &str,
    label: &str,
    interaction: ScheduleInteraction,
) {
    if event.target() != event.current_target()
        || !event.alt_key()
        || event.ctrl_key()
        || event.meta_key()
        || interaction.commands_disabled_untracked()
    {
        return;
    }
    let offset = match event.key().as_str() {
        "ArrowLeft" => -1,
        "ArrowRight" => 1,
        _ => return,
    };
    let Some(destination) = shift_iso_date(due_date, offset) else {
        interaction
            .announcement
            .set(format!("{label} has no date. Use its job actions menu."));
        return;
    };
    event.prevent_default();
    interaction
        .announcement
        .set(format!("{label} moving to {destination}."));
    interaction.schedule(id, destination);
}

fn shift_iso_date(value: &str, offset: i8) -> Option<String> {
    let mut parts = value.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let (mut year, mut month, mut day) = (
        year.parse::<i32>().ok()?,
        month.parse::<u8>().ok()?,
        day.parse::<u8>().ok()?,
    );
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    if offset < 0 {
        if day > 1 {
            day -= 1;
        } else if month > 1 {
            month -= 1;
            day = days_in_month(year, month);
        } else {
            year -= 1;
            month = 12;
            day = 31;
        }
    } else if day < days_in_month(year, month) {
        day += 1;
    } else if month < 12 {
        month += 1;
        day = 1;
    } else {
        year += 1;
        month = 1;
        day = 1;
    }
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

fn completed_ticket(job: JobView) -> impl IntoView {
    let element_id = format!("schedule-card-{}", job.id);
    let detail = format!("/jobs/{}", job.id);
    let detail_data = detail.clone();
    let detail_title = detail.clone();
    view! { <article class="job-ticket completed-ticket state-complete" data-job-id=job.id data-card-detail=detail_data><h3><a href=detail_title rel="external" data-detail-link id=element_id>{job.title}</a></h3><p class="ticket-customer">{job.customer_name}</p><div class="ticket-signals"><span class="pickup-signal">{if job.picked_up { "Picked up" } else { "Awaiting pickup" }}</span><span class="due-state due-archived">{job.archive_due_label}</span></div><a class="ticket-view" href=detail rel="external" data-detail-link>"View job"</a></article> }
}

fn schedule_focus(job_id: &str) {
    schedule_element_focus(format!("schedule-card-{job_id}"));
}

fn schedule_element_focus(element_id: impl Into<String>) {
    let element_id = element_id.into();
    leptos::leptos_dom::helpers::request_animation_frame(move || {
        let Some(element) = leptos::web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| {
                document
                    .get_element_by_id(&element_id)
                    .or_else(|| document.get_element_by_id("board-search"))
            })
            .and_then(|element| {
                wasm_bindgen::JsCast::dyn_into::<leptos::web_sys::HtmlElement>(element).ok()
            })
        else {
            return;
        };
        let _ = element.focus();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, due_date: &str) -> JobView {
        JobView {
            id: id.into(),
            due_date: due_date.into(),
            lifecycle: "active".into(),
            ..JobView::default()
        }
    }

    #[test]
    fn jobs_are_partitioned_by_due_date_without_workflow_status() {
        let days = vec![
            TimelineDayView {
                date: "2026-08-31".into(),
                weekday: "Mon".into(),
                label: "Aug 31".into(),
                is_today: true,
            },
            TimelineDayView {
                date: "2026-09-01".into(),
                weekday: "Tue".into(),
                label: "Sep 1".into(),
                is_today: false,
            },
        ];
        let columns = schedule_columns(
            vec![
                job("late", "2026-08-30"),
                job("today", "2026-08-31"),
                job("future", "2026-09-10"),
                job("none", ""),
            ],
            &days,
        );
        assert_eq!(
            columns
                .iter()
                .map(|column| column.key.as_str())
                .collect::<Vec<_>>(),
            ["2026-08-31", "2026-09-01", "later-or-undated"]
        );
        assert_eq!(columns[0].title, "Today & overdue");
        assert_eq!(columns[0].class, "is-today");
        assert_eq!(
            columns[0]
                .jobs
                .iter()
                .map(|job| job.id.as_str())
                .collect::<Vec<_>>(),
            ["late", "today"]
        );
        assert!(columns[1].jobs.is_empty());
        assert_eq!(
            columns
                .iter()
                .flat_map(|column| column.jobs.iter())
                .map(|job| job.id.as_str())
                .collect::<Vec<_>>(),
            ["late", "today", "future", "none"]
        );
    }

    #[test]
    fn schedule_shows_four_days_and_collects_everything_else_at_the_end() {
        let days = (3..=8)
            .map(|day| TimelineDayView {
                date: format!("2026-09-{day:02}"),
                weekday: format!("Day {day}"),
                label: format!("Sep {day}"),
                is_today: day == 3,
            })
            .collect::<Vec<_>>();
        let columns = schedule_columns(
            vec![
                job("late", "2026-09-02"),
                job("today", "2026-09-03"),
                job("fourth-day", "2026-09-06"),
                job("fifth-day", "2026-09-07"),
                job("undated", ""),
            ],
            &days,
        );

        assert_eq!(columns.len(), 5);
        assert_eq!(columns[0].drop_date.as_deref(), Some("2026-09-03"));
        assert_eq!(columns[3].drop_date.as_deref(), Some("2026-09-06"));
        assert!(
            columns[4].drop_date.is_none(),
            "The mixed later/undated lane must never clear a date implicitly"
        );
        assert_eq!(
            columns
                .iter()
                .map(|column| column.key.as_str())
                .collect::<Vec<_>>(),
            [
                "2026-09-03",
                "2026-09-04",
                "2026-09-05",
                "2026-09-06",
                "later-or-undated",
            ]
        );
        assert_eq!(
            columns[4]
                .jobs
                .iter()
                .map(|job| job.id.as_str())
                .collect::<Vec<_>>(),
            ["fifth-day", "undated"]
        );
    }

    #[test]
    fn drag_targets_only_change_when_the_due_date_changes() {
        let mut state = ScheduleDragState::default();
        state.start("job".into(), "2026-08-31".into());
        assert!(!state.hover("2026-08-31"));
        assert!(state.hover("2026-09-01"));
        assert!(state.is_target("2026-09-01"));
        state.finish();
        assert_eq!(state, ScheduleDragState::default());
    }

    #[test]
    fn keyboard_date_shift_handles_month_and_leap_year_boundaries() {
        assert_eq!(
            shift_iso_date("2026-08-31", 1).as_deref(),
            Some("2026-09-01")
        );
        assert_eq!(
            shift_iso_date("2024-03-01", -1).as_deref(),
            Some("2024-02-29")
        );
        assert_eq!(shift_iso_date("", 1), None);
    }

    #[test]
    fn card_states_use_glanceable_unambiguous_copy() {
        assert_eq!(card_status_label("design"), "Designing");
        assert_eq!(card_status_label("waiting"), "Waiting");
        assert_eq!(card_status_label("print"), "Print ready");
        assert_eq!(card_status_label("finish"), "Finish ready");
        assert_eq!(card_status_label("complete"), "Completed");
    }

    #[test]
    fn card_descriptors_omit_missing_information() {
        assert_eq!(descriptor_value(""), None);
        assert_eq!(descriptor_value("   "), None);
        assert_eq!(
            descriptor_value("  13 oz vinyl  ").as_deref(),
            Some("13 oz vinyl")
        );
        assert_eq!(billing_descriptor(None, None), None);
        assert_eq!(billing_descriptor(None, Some("  ")), None);
        assert_eq!(billing_descriptor(None, Some("invoice-1")), Some("Unpaid"));
        assert_eq!(
            billing_descriptor(Some("paid"), Some("invoice-1")),
            Some("Paid")
        );
    }
}
