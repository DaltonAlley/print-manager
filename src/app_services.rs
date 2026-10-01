use std::collections::{HashMap, HashSet};
#[cfg(test)]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    domain::{BoardLane, DocumentInput, DocumentKind, JobStage},
    email::ResendClient,
    read_models::{
        ARCHIVE_PAGE_SIZE, ArchiveData, ArchiveRequest, ArchiveTagView, AttachmentView,
        BoardCommandLane, BoardCommandOutcome, BoardCommandPlacement, BoardCommandRequest,
        BoardCommandResult, DetailTagView, DocumentFormData, DocumentSummary, JobDetailData,
        JobEventView, JobFormData, JobLanes, JobView, JobsData, LineForm, ScheduleJobOutcome,
        ScheduleJobRequest, ScheduleJobResult, SettingsData, SettingsTagView, TagView,
        TimelineDayView,
    },
    repository::{
        ArchiveFilter, BoardAdvance, BoardReorder, JobFilter, Repository, format_cents, format_rate,
    },
    storage::Storage,
};

/// Application query capabilities used by server-rendered presentation loaders.
///
/// Persistence records and multi-query orchestration stay behind this boundary;
/// callers receive application-owned read models ready for presentation.
#[derive(Clone)]
pub(crate) struct AppServices {
    repository: Repository,
    resend: Option<ResendClient>,
    storage: Storage,
    #[cfg(test)]
    fail_next_jobs_board_projection: Arc<AtomicBool>,
}

impl AppServices {
    #[must_use]
    pub(crate) fn new(
        repository: Repository,
        resend: Option<ResendClient>,
        storage: Storage,
    ) -> Self {
        Self {
            repository,
            resend,
            storage,
            #[cfg(test)]
            fail_next_jobs_board_projection: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    fn fail_next_jobs_board_projection(&self) {
        self.fail_next_jobs_board_projection
            .store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn check_jobs_board_projection(&self) -> Result<(), sqlx::Error> {
        if self
            .fail_next_jobs_board_projection
            .swap(false, Ordering::SeqCst)
        {
            return Err(sqlx::Error::Protocol(
                "injected jobs board projection failure".into(),
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) async fn jobs_board(
        &self,
        search: String,
        category: String,
        status: String,
        material: String,
        billing: String,
    ) -> Result<JobsData, sqlx::Error> {
        #[cfg(test)]
        self.check_jobs_board_projection()?;
        let filter = JobFilter {
            search,
            category,
            status,
            material,
            billing,
        };
        let archive_filter = ArchiveFilter::default();
        let (jobs, categories, materials, counts, archive_count, settings) = tokio::try_join!(
            self.repository.board_jobs(&filter),
            self.repository.categories(),
            self.repository.materials(),
            self.repository.job_counts(),
            self.repository.archived_job_count(&archive_filter),
            self.repository.settings()
        )?;
        let job_ids = jobs.iter().map(|job| job.id.clone()).collect::<Vec<_>>();
        let (tag_rows, attachment_rows) = tokio::try_join!(
            self.repository.job_tag_projections(&job_ids),
            self.repository.job_attachment_projections(&job_ids)
        )?;
        let mut tags: HashMap<String, Vec<TagView>> = HashMap::new();
        for tag in tag_rows {
            tags.entry(tag.job_id).or_default().push(TagView {
                id: tag.id,
                name: tag.name,
                color_key: tag.color_key,
            });
        }
        let mut attachments = HashMap::<String, Vec<AttachmentView>>::new();
        for row in attachment_rows {
            attachments
                .entry(row.job_id)
                .or_default()
                .push(AttachmentView {
                    id: row.id,
                    name: row.original_name,
                    media_type: row.media_type,
                    byte_size: row.byte_size,
                });
        }
        let today_date = self.repository.business_today();
        let today = today_date.to_string();
        let turnaround_days = usize::try_from(settings.default_turnaround_days)
            .unwrap_or(3)
            .clamp(1, 365);
        let timeline_day_count = turnaround_days.saturating_add(7).clamp(14, 31);
        let timeline_days = (0..timeline_day_count)
            .filter_map(|offset| {
                today_date
                    .checked_add_days(chrono::Days::new(u64::try_from(offset).ok()?))
                    .map(|date| TimelineDayView {
                        date: date.to_string(),
                        weekday: date.format("%a").to_string(),
                        label: date.format("%b %-d").to_string(),
                        is_today: offset == 0,
                    })
            })
            .collect::<Vec<_>>();
        let mut jobs: Vec<JobView> = jobs
            .into_iter()
            .map(|job| {
                let lifecycle = if job.completed_at.is_none() {
                    "active"
                } else {
                    "complete"
                };
                let due_date = job.due_date.unwrap_or_default();
                let due = due_state(&due_date, &today);
                let (timeline_start, timeline_span) =
                    timeline_position(&due_date, today_date, turnaround_days, timeline_day_count);
                let id = job.id;
                let attachments = attachments.remove(&id).unwrap_or_default();
                JobView {
                    tags: tags.remove(&id).unwrap_or_default(),
                    attachment_count: attachments.len(),
                    attachments,
                    id,
                    title: job.title,
                    customer_name: job.customer_name,
                    category: job.category,
                    material: job.material,
                    status: job.status,
                    due_date,
                    due_label: due.label,
                    due_class: due.class.into(),
                    archive_due_label: due.archive_label,
                    invoice_id: job.invoice_id,
                    invoice_status: job.invoice_status,
                    picked_up: job.picked_up_at.is_some(),
                    lifecycle: lifecycle.into(),
                    timeline_start,
                    timeline_span,
                }
            })
            .collect();
        jobs.sort_by(|left, right| {
            (left.lifecycle != "active")
                .cmp(&(right.lifecycle != "active"))
                .then_with(|| left.due_date.is_empty().cmp(&right.due_date.is_empty()))
                .then_with(|| left.due_date.cmp(&right.due_date))
        });
        Ok(JobsData {
            lanes: group_jobs(jobs.clone()),
            jobs,
            categories,
            materials,
            total_jobs: counts.total,
            active_jobs: counts.active,
            archive_count,
            timeline_days,
            default_turnaround_days: turnaround_days,
        })
    }

    /// Moves an active job to one scheduling date, then returns the canonical board projection.
    pub(crate) async fn schedule_job(
        &self,
        request: ScheduleJobRequest,
    ) -> Result<Option<ScheduleJobResult>, sqlx::Error> {
        let due_date = request.due_date.trim();
        if !due_date.is_empty() && chrono::NaiveDate::parse_from_str(due_date, "%Y-%m-%d").is_err()
        {
            return Ok(None);
        }
        let due_date = (!due_date.is_empty()).then_some(due_date);
        if !self
            .repository
            .schedule_job(&request.job_id, due_date)
            .await?
        {
            return Ok(None);
        }
        let outcome = ScheduleJobOutcome {
            job_id: request.job_id,
            due_date: due_date.unwrap_or_default().to_owned(),
        };
        let result = match self
            .jobs_board(
                request.search,
                request.category,
                request.status,
                request.material,
                request.billing,
            )
            .await
        {
            Ok(data) => ScheduleJobResult::Refreshed {
                data: Box::new(data),
                outcome,
            },
            Err(_) => ScheduleJobResult::CommittedRefreshFailed { outcome },
        };
        Ok(Some(result))
    }

    /// Applies exactly one transactional board mutation, then reports either
    /// the canonical filtered projection or that the committed mutation needs
    /// a read-only projection recovery.
    pub(crate) async fn execute_board_command(
        &self,
        request: BoardCommandRequest,
    ) -> Result<Option<BoardCommandResult>, sqlx::Error> {
        let command = request.command;
        let source = board_lane(command.source);
        let destination = board_lane(command.destination);
        let outcome = if source == destination {
            let reorder = match &command.placement {
                BoardCommandPlacement::Before(target) => BoardReorder::Before(target),
                BoardCommandPlacement::After(target) => BoardReorder::After(target),
                BoardCommandPlacement::Last => BoardReorder::Last,
                BoardCommandPlacement::Up => BoardReorder::Up,
                BoardCommandPlacement::Down => BoardReorder::Down,
            };
            if !self
                .repository
                .reorder_board_card(source, &command.card_id, reorder)
                .await?
            {
                return Ok(None);
            }
            BoardCommandOutcome::Sorted
        } else {
            let reorder = match &command.placement {
                BoardCommandPlacement::Before(target) => BoardReorder::Before(target),
                BoardCommandPlacement::After(target) => BoardReorder::After(target),
                BoardCommandPlacement::Last => BoardReorder::Last,
                BoardCommandPlacement::Up | BoardCommandPlacement::Down => return Ok(None),
            };
            match self
                .repository
                .move_board_card(source, destination, &command.card_id, reorder)
                .await?
            {
                Some(BoardAdvance::StartedJob(job_id)) => BoardCommandOutcome::Started(job_id),
                Some(BoardAdvance::Moved(stage)) => BoardCommandOutcome::Moved(board_stage(stage)),
                None => return Ok(None),
            }
        };
        let result = match self
            .jobs_board(
                request.search,
                request.category,
                request.status,
                request.material,
                request.billing,
            )
            .await
        {
            Ok(data) => BoardCommandResult::Refreshed {
                data: Box::new(data),
                outcome,
            },
            Err(_) => BoardCommandResult::CommittedRefreshFailed { outcome },
        };
        Ok(Some(result))
    }

    pub(crate) async fn job_archive(
        &self,
        request: ArchiveRequest,
    ) -> Result<ArchiveData, sqlx::Error> {
        let requested_page = request.page.max(1);
        let filter = ArchiveFilter {
            search: request.search,
            invoice: request.invoice,
            pickup: request.pickup,
            completed_from: request.completed_from,
            completed_through: request.completed_through,
            category: request.category,
            tag_id: request.tag_id,
        };
        let (total, categories, available_tags) = tokio::try_join!(
            self.repository.archived_job_count(&filter),
            self.repository.categories(),
            self.repository.tags(true),
        )?;
        let page = requested_page.min(total.div_ceil(ARCHIVE_PAGE_SIZE).max(1));
        let offset =
            u32::try_from((page - 1).saturating_mul(ARCHIVE_PAGE_SIZE)).unwrap_or(u32::MAX);
        let jobs = self
            .repository
            .filtered_archived_jobs(
                &filter,
                offset,
                u32::try_from(ARCHIVE_PAGE_SIZE).unwrap_or(u32::MAX),
            )
            .await?;
        let ids = jobs.iter().map(|job| job.id.clone()).collect::<Vec<_>>();
        let (tag_rows, attachment_rows) = tokio::try_join!(
            self.repository.job_tag_projections(&ids),
            self.repository.job_attachment_counts(&ids)
        )?;
        let mut tags: HashMap<String, Vec<TagView>> = HashMap::new();
        for tag in tag_rows {
            tags.entry(tag.job_id).or_default().push(TagView {
                id: tag.id,
                name: tag.name,
                color_key: tag.color_key,
            });
        }
        let attachment_counts = attachment_rows
            .into_iter()
            .map(|row| {
                (
                    row.job_id,
                    usize::try_from(row.attachment_count).unwrap_or(usize::MAX),
                )
            })
            .collect::<HashMap<_, _>>();
        let today = self.repository.business_today().to_string();
        let jobs = jobs
            .into_iter()
            .map(|job| {
                let due_date = job.due_date.unwrap_or_default();
                let due = due_state(&due_date, &today);
                let id = job.id;
                JobView {
                    tags: tags.remove(&id).unwrap_or_default(),
                    attachment_count: attachment_counts.get(&id).copied().unwrap_or_default(),
                    attachments: Vec::new(),
                    id,
                    title: job.title,
                    customer_name: job.customer_name,
                    category: job.category,
                    material: job.material,
                    status: job.status,
                    due_date,
                    due_label: due.label,
                    due_class: due.class.into(),
                    archive_due_label: due.archive_label,
                    invoice_id: job.invoice_id,
                    invoice_status: job.invoice_status,
                    picked_up: job.picked_up_at.is_some(),
                    lifecycle: "archive".into(),
                    timeline_start: 0,
                    timeline_span: 1,
                }
            })
            .collect();
        Ok(ArchiveData {
            jobs,
            total,
            page,
            categories,
            tags: available_tags
                .into_iter()
                .map(|tag| ArchiveTagView {
                    id: tag.id,
                    name: tag.name,
                    color_key: tag.color_key,
                })
                .collect(),
        })
    }

    pub(crate) async fn job_editor(
        &self,
        id: Option<String>,
        quote: Option<String>,
    ) -> Result<Option<JobFormData>, sqlx::Error> {
        if let Some(id) = id {
            return Ok(self.repository.job(&id).await?.map(|job| JobFormData {
                id: Some(id),
                title: job.title,
                description: job.description,
                price: format_cents(job.price_cents),
                customer_name: job.customer_name,
                customer_phone: job.customer_phone,
                customer_email: job.customer_email,
                category: job.category,
                material: job.material,
                status: job.status,
                due_date: job.due_date.unwrap_or_default(),
                notes: job.notes,
                quote_id: job.quote_id,
                quote_number: None,
                errors: Vec::new(),
            }));
        }
        let mut data = JobFormData {
            status: "design".into(),
            ..JobFormData::default()
        };
        let settings = self.repository.settings().await?;
        let turnaround_days = u64::try_from(settings.default_turnaround_days).unwrap_or(3);
        data.due_date = self
            .repository
            .business_today()
            .checked_add_days(chrono::Days::new(turnaround_days))
            .map(|date| date.to_string())
            .unwrap_or_default();
        if let Some(quote_id) = quote
            && let Some(doc) = self
                .repository
                .document(&quote_id, DocumentKind::Quote)
                .await?
        {
            let quote_number = doc.document.number.clone();
            data.title = format!("Quote {quote_number}");
            data.description = doc
                .lines
                .iter()
                .map(|line| line.description.trim())
                .filter(|description| !description.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            data.price = format_cents(doc.total_cents());
            data.customer_name = doc.document.customer_name;
            data.customer_phone = doc.document.customer_phone;
            data.customer_email = doc.document.customer_email;
            data.quote_id = Some(quote_id);
            data.quote_number = Some(quote_number);
        }
        Ok(Some(data))
    }

    pub(crate) async fn job_detail(&self, id: &str) -> Result<Option<JobDetailData>, sqlx::Error> {
        let Some(job) = self.repository.job(id).await? else {
            return Ok(None);
        };
        let (assigned, mut all_tags, attachments, events) = tokio::try_join!(
            self.repository.tags_for_job(id),
            self.repository.tags(false),
            self.repository.attachments_for_job(id),
            self.repository.job_events(id),
        )?;
        let assigned_ids = assigned
            .iter()
            .map(|tag| tag.id.clone())
            .collect::<HashSet<_>>();
        for tag in assigned {
            if !all_tags.iter().any(|candidate| candidate.id == tag.id) {
                all_tags.push(tag);
            }
        }
        let invoice = if let Some(invoice_id) = job.invoice_id.as_deref() {
            self.repository
                .document(invoice_id, DocumentKind::Invoice)
                .await?
                .map(|doc| self.document_read_model(&doc, DocumentKind::Invoice, false))
        } else {
            None
        };
        Ok(Some(JobDetailData {
            id: job.id,
            title: job.title,
            description: job.description,
            price_cents: job.price_cents,
            customer_name: job.customer_name,
            customer_phone: job.customer_phone,
            customer_email: job.customer_email,
            category: job.category,
            material: job.material,
            status: job.status,
            due_date: job.due_date.unwrap_or_default(),
            notes: job.notes,
            active: job.completed_at.is_none(),
            picked_up: job.picked_up_at.is_some(),
            invoice_status: job.invoice_status,
            tags: all_tags
                .into_iter()
                .map(|tag| DetailTagView {
                    assigned: assigned_ids.contains(&tag.id),
                    id: tag.id,
                    name: tag.name,
                    color_key: tag.color_key,
                })
                .collect(),
            attachments: attachments
                .into_iter()
                .map(|file| AttachmentView {
                    id: file.id,
                    name: file.original_name,
                    media_type: file.media_type,
                    byte_size: file.byte_size,
                })
                .collect(),
            invoice,
            events: events
                .into_iter()
                .map(|event| JobEventView {
                    kind: event.kind,
                    summary: event.summary,
                    occurred_at: event.occurred_at,
                })
                .collect(),
        }))
    }

    pub(crate) async fn document_list(
        &self,
        kind: DocumentKind,
        search: String,
    ) -> Result<Vec<DocumentSummary>, sqlx::Error> {
        Ok(self
            .repository
            .search_documents(kind, &search)
            .await?
            .into_iter()
            .map(|doc| DocumentSummary {
                total_cents: doc.total_cents(),
                id: doc.document.id,
                number: doc.document.number,
                customer_name: doc.document.customer_name,
                status: doc.document.status,
                date: doc.document.expiry_or_due_date.unwrap_or_default(),
                updated_at: doc.document.updated_at,
            })
            .collect())
    }

    pub(crate) async fn document_editor(
        &self,
        kind: DocumentKind,
        id: Option<String>,
    ) -> Result<Option<DocumentFormData>, sqlx::Error> {
        if let Some(id) = id {
            return Ok(self
                .repository
                .document(&id, kind)
                .await?
                .map(|doc| self.document_read_model(&doc, kind, true)));
        }
        let settings = self.repository.settings().await?;
        let input = DocumentInput {
            issue_date: self.repository.business_today().to_string(),
            tax_rate: format_rate(settings.default_tax_basis_points),
            ..DocumentInput::default()
        };
        Ok(Some(
            self.document_input_read_model(None, None, kind, input),
        ))
    }

    pub(crate) async fn settings(&self) -> Result<SettingsData, sqlx::Error> {
        let (value, tags) =
            tokio::try_join!(self.repository.settings(), self.repository.tags(false))?;
        Ok(SettingsData {
            business_name: value.business_name,
            address: value.address,
            phone: value.phone,
            email: value.email,
            has_logo: value
                .logo_path
                .as_deref()
                .is_some_and(|identifier| self.storage.managed_logo_path(identifier).is_ok()),
            default_tax_rate: format_rate(value.default_tax_basis_points),
            default_turnaround_days: value.default_turnaround_days.to_string(),
            quote_prefix: value.quote_prefix,
            invoice_prefix: value.invoice_prefix,
            resend_configured: self.resend.is_some(),
            errors: Vec::new(),
            tags: tags
                .into_iter()
                .map(|tag| SettingsTagView {
                    id: tag.id,
                    name: tag.name,
                    color_key: tag.color_key,
                })
                .collect(),
        })
    }

    fn document_read_model(
        &self,
        document: &crate::repository::FullDocument,
        kind: DocumentKind,
        include_lines: bool,
    ) -> DocumentFormData {
        let id = document.document.id.clone();
        let number = document.document.number.clone();
        let mut value =
            self.document_input_read_model(Some(id), Some(number), kind, document.to_input());
        if !include_lines {
            value.discount.clear();
            value.lines.clear();
        }
        value
    }

    fn document_input_read_model(
        &self,
        id: Option<String>,
        number: Option<String>,
        kind: DocumentKind,
        input: DocumentInput,
    ) -> DocumentFormData {
        DocumentFormData {
            id,
            number,
            kind: kind.as_str().into(),
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
            resend_configured: self.resend.is_some(),
            errors: Vec::new(),
            email: None,
        }
    }
}

fn board_lane(lane: BoardCommandLane) -> BoardLane {
    match lane {
        BoardCommandLane::Quote => BoardLane::Quote,
        BoardCommandLane::Design => BoardLane::Production(JobStage::Design),
        BoardCommandLane::Waiting => BoardLane::Production(JobStage::Waiting),
        BoardCommandLane::Print => BoardLane::Production(JobStage::Print),
        BoardCommandLane::Finish => BoardLane::Production(JobStage::Finish),
        BoardCommandLane::Complete => BoardLane::Production(JobStage::Complete),
    }
}

fn board_stage(stage: JobStage) -> BoardCommandLane {
    match stage {
        JobStage::Design => BoardCommandLane::Design,
        JobStage::Waiting => BoardCommandLane::Waiting,
        JobStage::Print => BoardCommandLane::Print,
        JobStage::Finish => BoardCommandLane::Finish,
        JobStage::Complete => BoardCommandLane::Complete,
    }
}

fn group_jobs(jobs: Vec<JobView>) -> JobLanes {
    let mut lanes = JobLanes::default();
    for job in jobs {
        let lane = if job.lifecycle == "complete" || job.status == "complete" {
            &mut lanes.complete
        } else {
            match job.status.as_str() {
                "waiting" => &mut lanes.waiting,
                "print" => &mut lanes.print,
                "finish" => &mut lanes.finish,
                _ => &mut lanes.design,
            }
        };
        lane.push(job);
    }
    lanes
}

fn timeline_position(
    due_date: &str,
    today: chrono::NaiveDate,
    turnaround_days: usize,
    visible_days: usize,
) -> (usize, usize) {
    let last_column = visible_days.saturating_sub(1);
    let due = chrono::NaiveDate::parse_from_str(due_date, "%Y-%m-%d").unwrap_or_else(|_| {
        today
            .checked_add_days(chrono::Days::new(
                u64::try_from(turnaround_days).unwrap_or(1),
            ))
            .unwrap_or(today)
    });
    let duration_before_due = u64::try_from(turnaround_days).unwrap_or(1);
    let start = due
        .checked_sub_days(chrono::Days::new(duration_before_due))
        .unwrap_or(due);
    let last_column = i64::try_from(last_column).unwrap_or(i64::MAX);
    let start_offset = (start - today).num_days().clamp(0, last_column);
    let end_offset = (due - today).num_days().clamp(start_offset, last_column);
    let start = usize::try_from(start_offset).unwrap_or(0);
    let end = usize::try_from(end_offset).unwrap_or(start);
    (start, end.saturating_sub(start).max(1))
}

#[derive(Debug, PartialEq, Eq)]
struct DuePresentation {
    label: String,
    class: &'static str,
    archive_label: String,
}

fn due_state(due_date: &str, today: &str) -> DuePresentation {
    if due_date.is_empty() {
        return DuePresentation {
            label: "No due date".into(),
            class: "due-missing",
            archive_label: "No due date".into(),
        };
    }
    let display_date = friendly_date(due_date, today);
    let (label, class) = match due_date.cmp(today) {
        std::cmp::Ordering::Less => (format!("Overdue · {display_date}"), "due-overdue"),
        std::cmp::Ordering::Equal => ("Due today".into(), "due-today"),
        std::cmp::Ordering::Greater => (format!("Due {display_date}"), "due-upcoming"),
    };
    DuePresentation {
        label,
        class,
        archive_label: format!("Due {display_date}"),
    }
}

fn friendly_date(date: &str, today: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = date.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return date.into();
    };
    let Some(month_name) = month
        .parse::<usize>()
        .ok()
        .and_then(|number| number.checked_sub(1))
        .and_then(|index| MONTHS.get(index))
    else {
        return date.into();
    };
    let Ok(day_number) = day.parse::<u8>() else {
        return date.into();
    };
    if today.starts_with(year) {
        format!("{month_name} {day_number}")
    } else {
        format!("{month_name} {day_number}, {year}")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{AppServices, due_state, group_jobs, timeline_position};
    use crate::{
        database::Database,
        domain::{DocumentInput, DocumentKind, JobInput, LineInput},
        read_models::{
            BoardCommand, BoardCommandLane, BoardCommandOutcome, BoardCommandPlacement,
            BoardCommandRequest, BoardCommandResult, JobView, ScheduleJobRequest,
            ScheduleJobResult,
        },
        repository::Repository,
        storage::Storage,
    };

    fn job(id: &str, status: &str, lifecycle: &str) -> JobView {
        JobView {
            id: id.into(),
            status: status.into(),
            lifecycle: lifecycle.into(),
            ..JobView::default()
        }
    }

    #[test]
    fn jobs_are_grouped_by_board_phase_without_reordering() {
        let lanes = group_jobs(vec![
            job("print-first", "print", "active"),
            job("design-first", "design", "active"),
            job("design-second", "design", "active"),
            job("waiting", "waiting", "active"),
            job("finished", "complete", "complete"),
            job("exception", "finish", "complete"),
        ]);

        assert_eq!(
            lanes
                .design
                .iter()
                .map(|job| job.id.as_str())
                .collect::<Vec<_>>(),
            ["design-first", "design-second"]
        );
        assert_eq!(lanes.waiting[0].id, "waiting");
        assert_eq!(lanes.print[0].id, "print-first");
        assert!(lanes.finish.is_empty());
        assert_eq!(
            lanes
                .complete
                .iter()
                .map(|job| job.id.as_str())
                .collect::<Vec<_>>(),
            ["finished", "exception"]
        );
    }

    #[test]
    fn unknown_active_stage_uses_the_design_lane() {
        let lanes = group_jobs(vec![job("legacy", "legacy-stage", "active")]);

        assert_eq!(lanes.design[0].id, "legacy");
        assert!(lanes.waiting.is_empty());
        assert!(lanes.print.is_empty());
        assert!(lanes.finish.is_empty());
        assert!(lanes.complete.is_empty());
    }

    #[test]
    fn due_state_derives_board_and_archive_labels() {
        let overdue = due_state("2026-08-20", "2026-08-21");
        assert_eq!(overdue.label, "Overdue · Aug 20");
        assert_eq!(overdue.class, "due-overdue");
        assert_eq!(overdue.archive_label, "Due Aug 20");
        let today = due_state("2026-08-21", "2026-08-21");
        assert_eq!(today.label, "Due today");
        assert_eq!(today.archive_label, "Due Aug 21");
        let future_year = due_state("2027-01-03", "2026-08-21");
        assert_eq!(future_year.label, "Due Jan 3, 2027");
        assert_eq!(due_state("", "2026-08-21").label, "No due date");
    }

    #[test]
    fn timeline_positions_end_on_due_dates_and_stay_inside_the_visible_guide() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 8, 31)
            .unwrap_or_else(|| panic!("valid fixture date"));
        assert_eq!(timeline_position("2026-09-03", today, 3, 14), (0, 3));
        assert_eq!(timeline_position("2026-08-20", today, 3, 14), (0, 1));
        assert_eq!(timeline_position("2026-10-20", today, 3, 14), (13, 1));
    }

    #[tokio::test]
    async fn board_command_mutates_once_and_returns_the_filtered_canonical_snapshot() {
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
                    title: "Canonical card".into(),
                    customer_name: "Filtered customer".into(),
                    category: "Cards".into(),
                    ..JobInput::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let services = AppServices::new(
            repository.clone(),
            None,
            Storage::new(PathBuf::from("/tmp/print-manager-board-test"), 1024),
        );

        let result = services
            .execute_board_command(BoardCommandRequest {
                command: BoardCommand {
                    source: BoardCommandLane::Design,
                    destination: BoardCommandLane::Print,
                    card_id: job_id.clone(),
                    placement: BoardCommandPlacement::Last,
                },
                search: "Filtered customer".into(),
                category: "Cards".into(),
                status: "print".into(),
                material: String::new(),
                billing: String::new(),
            })
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!());

        let BoardCommandResult::Refreshed { data, outcome } = result else {
            panic!("normal command should return the canonical snapshot");
        };
        assert_eq!(outcome, BoardCommandOutcome::Moved(BoardCommandLane::Print));
        assert!(data.lanes.design.is_empty());
        assert_eq!(data.lanes.print.len(), 1);
        assert_eq!(data.lanes.print[0].id, job_id);
        assert_eq!(
            repository
                .job(&data.lanes.print[0].id)
                .await
                .unwrap_or(None)
                .map(|job| job.status),
            Some("print".into())
        );
        let detail = services
            .job_detail(&job_id)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!());
        assert_eq!(
            detail
                .events
                .iter()
                .map(|event| event.summary.as_str())
                .collect::<Vec<_>>(),
            ["Moved from Design to Print", "Job created"]
        );
    }

    #[tokio::test]
    async fn committed_board_command_recovers_projection_without_repeating_mutation() {
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
                    title: "Recoverable card".into(),
                    customer_name: "Projection customer".into(),
                    category: "Cards".into(),
                    ..JobInput::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let services = AppServices::new(
            repository.clone(),
            None,
            Storage::new(
                PathBuf::from("/tmp/print-manager-board-recovery-test"),
                1024,
            ),
        );
        let request = BoardCommandRequest {
            command: BoardCommand {
                source: BoardCommandLane::Design,
                destination: BoardCommandLane::Print,
                card_id: job_id.clone(),
                placement: BoardCommandPlacement::Last,
            },
            search: "Projection customer".into(),
            category: "Cards".into(),
            status: "print".into(),
            material: String::new(),
            billing: String::new(),
        };

        services.fail_next_jobs_board_projection();
        let result = services
            .execute_board_command(request.clone())
            .await
            .unwrap_or_else(|error| panic!("mutation unexpectedly failed: {error}"))
            .unwrap_or_else(|| panic!("mutation unexpectedly rejected"));

        assert!(matches!(
            result,
            BoardCommandResult::CommittedRefreshFailed {
                outcome: BoardCommandOutcome::Moved(BoardCommandLane::Print)
            }
        ));
        assert_eq!(
            repository
                .job(&job_id)
                .await
                .unwrap_or(None)
                .map(|job| job.status),
            Some("print".into())
        );

        let recovered = services
            .jobs_board(
                request.search.clone(),
                request.category.clone(),
                request.status.clone(),
                request.material.clone(),
                request.billing.clone(),
            )
            .await
            .unwrap_or_else(|error| panic!("projection recovery failed: {error}"));
        assert!(recovered.lanes.design.is_empty());
        assert_eq!(recovered.lanes.print.len(), 1);
        assert_eq!(recovered.lanes.print[0].id, job_id);

        assert!(
            services
                .execute_board_command(request)
                .await
                .unwrap_or_else(|error| panic!("retry should be rejected, not fail: {error}"))
                .is_none(),
            "the already-committed mutation cannot validly execute a second time"
        );
    }

    #[tokio::test]
    async fn schedule_command_changes_only_an_active_jobs_due_date_and_returns_due_order() {
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
        let later = repository
            .save_job(
                None,
                &JobInput {
                    title: "Later".into(),
                    customer_name: "Schedule customer".into(),
                    category: "Cards".into(),
                    due_date: "2026-09-10".into(),
                    ..JobInput::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let earlier = repository
            .save_job(
                None,
                &JobInput {
                    title: "Earlier".into(),
                    customer_name: "Schedule customer".into(),
                    category: "Cards".into(),
                    due_date: "2026-09-05".into(),
                    ..JobInput::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let services = AppServices::new(
            repository.clone(),
            None,
            Storage::new(PathBuf::from("/tmp/print-manager-schedule-test"), 1024),
        );

        let result = services
            .schedule_job(ScheduleJobRequest {
                job_id: later.clone(),
                due_date: "2026-09-01".into(),
                search: "Schedule customer".into(),
                category: "Cards".into(),
                status: String::new(),
                material: String::new(),
                billing: String::new(),
            })
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("valid schedule move was rejected"));
        let ScheduleJobResult::Refreshed { data, outcome } = result else {
            panic!("normal schedule move should refresh");
        };
        assert_eq!(outcome.job_id, later);
        assert_eq!(outcome.due_date, "2026-09-01");
        assert_eq!(
            data.jobs
                .iter()
                .map(|job| job.id.as_str())
                .collect::<Vec<_>>(),
            [later.as_str(), earlier.as_str()]
        );
        assert_eq!(
            repository
                .job(&later)
                .await
                .unwrap_or(None)
                .and_then(|job| job.due_date),
            Some("2026-09-01".into())
        );
        assert!(
            services
                .schedule_job(ScheduleJobRequest {
                    job_id: later,
                    due_date: "not-a-date".into(),
                    search: String::new(),
                    category: String::new(),
                    status: String::new(),
                    material: String::new(),
                    billing: String::new(),
                })
                .await
                .unwrap_or_else(|error| panic!("{error}"))
                .is_none()
        );
    }

    #[tokio::test]
    async fn quote_contact_prefills_editor_and_propagates_to_started_job() {
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
            customer_name: "Quote customer".into(),
            customer_phone: "+1 555 0199".into(),
            customer_email: "quote@example.test".into(),
            issue_date: "2026-08-24".into(),
            lines: vec![LineInput {
                description: "Posters".into(),
                quantity: "1".into(),
                unit_label: "job".into(),
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
        let services = AppServices::new(
            repository.clone(),
            None,
            Storage::new(PathBuf::from("/tmp/print-manager-quote-contact-test"), 1024),
        );

        let editor = services
            .job_editor(None, Some(quote_id.clone()))
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!());
        assert_eq!(editor.customer_phone, "+1 555 0199");
        assert_eq!(editor.customer_email, "quote@example.test");

        let job_id = repository
            .start_job_from_quote(&quote_id)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!());
        let job = repository
            .job(&job_id)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!());
        assert_eq!(job.customer_phone, "+1 555 0199");
        assert_eq!(job.customer_email, "quote@example.test");
        assert!(
            job.due_date
                .as_deref()
                .is_some_and(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok()),
            "quote-created jobs receive a valid default due date"
        );
    }
}
