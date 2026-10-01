use serde::{Deserialize, Serialize};

pub(crate) const DOCUMENT_LOAD_FAILURE_OUTCOME: &str = "repository";
pub const ARCHIVE_PAGE_SIZE: usize = 24;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TagView {
    pub id: String,
    pub name: String,
    pub color_key: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AttachmentView {
    pub id: String,
    pub name: String,
    pub media_type: String,
    pub byte_size: i64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct JobView {
    pub id: String,
    pub title: String,
    pub customer_name: String,
    pub category: String,
    pub material: String,
    pub status: String,
    pub due_date: String,
    pub due_label: String,
    pub due_class: String,
    pub archive_due_label: String,
    pub invoice_id: Option<String>,
    pub invoice_status: Option<String>,
    pub picked_up: bool,
    pub attachment_count: usize,
    pub attachments: Vec<AttachmentView>,
    pub tags: Vec<TagView>,
    pub lifecycle: String,
    pub timeline_start: usize,
    pub timeline_span: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct JobsData {
    /// Canonical board order: due date first, with stable repository tie-breakers.
    pub jobs: Vec<JobView>,
    pub lanes: JobLanes,
    pub categories: Vec<String>,
    pub materials: Vec<String>,
    pub total_jobs: usize,
    pub active_jobs: usize,
    pub archive_count: usize,
    pub timeline_days: Vec<TimelineDayView>,
    pub default_turnaround_days: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScheduleJobRequest {
    pub job_id: String,
    /// An ISO calendar date, or an empty string to return the job to No due date.
    pub due_date: String,
    pub search: String,
    pub category: String,
    pub status: String,
    pub material: String,
    pub billing: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScheduleJobOutcome {
    pub job_id: String,
    pub due_date: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ScheduleJobResult {
    Refreshed {
        data: Box<JobsData>,
        outcome: ScheduleJobOutcome,
    },
    CommittedRefreshFailed {
        outcome: ScheduleJobOutcome,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TimelineDayView {
    pub date: String,
    pub weekday: String,
    pub label: String,
    pub is_today: bool,
}

/// A single user intent against the production board.
///
/// This is shared by the hydrated board and its server function so browser
/// interactions cannot drift from the repository mutation contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BoardCommand {
    pub source: BoardCommandLane,
    pub destination: BoardCommandLane,
    pub card_id: String,
    pub placement: BoardCommandPlacement,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BoardCommandLane {
    Quote,
    Design,
    Waiting,
    Print,
    Finish,
    Complete,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BoardCommandPlacement {
    Before(String),
    After(String),
    Last,
    Up,
    Down,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BoardCommandOutcome {
    Started(String),
    Moved(BoardCommandLane),
    Sorted,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum BoardCommandResult {
    Refreshed {
        data: Box<JobsData>,
        outcome: BoardCommandOutcome,
    },
    CommittedRefreshFailed {
        outcome: BoardCommandOutcome,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BoardCommandRequest {
    pub command: BoardCommand,
    pub search: String,
    pub category: String,
    pub status: String,
    pub material: String,
    pub billing: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct JobLanes {
    pub design: Vec<JobView>,
    pub waiting: Vec<JobView>,
    pub print: Vec<JobView>,
    pub finish: Vec<JobView>,
    pub complete: Vec<JobView>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ArchiveData {
    pub jobs: Vec<JobView>,
    pub total: usize,
    pub page: usize,
    pub categories: Vec<String>,
    pub tags: Vec<ArchiveTagView>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArchiveRequest {
    pub search: String,
    pub invoice: String,
    pub pickup: String,
    pub completed_from: String,
    pub completed_through: String,
    pub category: String,
    pub tag_id: String,
    pub page: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ArchiveTagView {
    pub id: String,
    pub name: String,
    pub color_key: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct JobFormData {
    pub id: Option<String>,
    pub title: String,
    pub description: String,
    pub price: String,
    pub customer_name: String,
    pub customer_phone: String,
    pub customer_email: String,
    pub category: String,
    pub material: String,
    pub status: String,
    pub due_date: String,
    pub notes: String,
    pub quote_id: Option<String>,
    pub quote_number: Option<String>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct DetailTagView {
    pub id: String,
    pub name: String,
    pub color_key: String,
    pub assigned: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobEventView {
    pub kind: String,
    pub summary: String,
    pub occurred_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct JobDetailData {
    pub id: String,
    pub title: String,
    pub description: String,
    pub price_cents: i64,
    pub customer_name: String,
    pub customer_phone: String,
    pub customer_email: String,
    pub category: String,
    pub material: String,
    pub status: String,
    pub due_date: String,
    pub notes: String,
    pub active: bool,
    pub picked_up: bool,
    pub invoice_status: Option<String>,
    pub tags: Vec<DetailTagView>,
    pub attachments: Vec<AttachmentView>,
    pub invoice: Option<DocumentFormData>,
    pub events: Vec<JobEventView>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct DocumentSummary {
    pub id: String,
    pub number: String,
    pub customer_name: String,
    pub total_cents: i64,
    pub status: String,
    pub date: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LineForm {
    pub description: String,
    pub quantity: String,
    pub unit_label: String,
    pub unit_price: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct DocumentFormData {
    pub id: Option<String>,
    pub number: Option<String>,
    pub kind: String,
    pub status: String,
    pub customer_name: String,
    pub customer_email: String,
    pub customer_phone: String,
    pub billing_address: String,
    pub issue_date: String,
    pub date: String,
    pub discount: String,
    pub tax_rate: String,
    pub notes: String,
    pub terms: String,
    pub lines: Vec<LineForm>,
    pub resend_configured: bool,
    pub errors: Vec<String>,
    pub email: Option<EmailFormData>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct EmailFormData {
    pub recipient: String,
    pub subject: String,
    pub message: String,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SettingsData {
    pub business_name: String,
    pub address: String,
    pub phone: String,
    pub email: String,
    pub has_logo: bool,
    pub default_tax_rate: String,
    pub default_turnaround_days: String,
    pub quote_prefix: String,
    pub invoice_prefix: String,
    pub resend_configured: bool,
    pub errors: Vec<String>,
    pub tags: Vec<SettingsTagView>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SettingsTagView {
    pub id: String,
    pub name: String,
    pub color_key: String,
}
