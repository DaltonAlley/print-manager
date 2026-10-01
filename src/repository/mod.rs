#![allow(clippy::missing_errors_doc)]

mod attachments;
mod board;
mod documents;
mod events;
mod jobs;
mod settings;
mod tags;
mod workflow;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use sqlx::{FromRow, SqlitePool};

use crate::{
    calculation::{LineCalculationInput, Money, calculate_document},
    domain::{DocumentInput, JobStage, LineInput},
};

pub use documents::{format_cents, format_rate};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoardReorder<'a> {
    Up,
    Down,
    Last,
    Before(&'a str),
    After(&'a str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BoardAdvance {
    StartedJob(String),
    Moved(JobStage),
}

#[derive(Clone, Debug, FromRow)]
pub struct Settings {
    pub business_name: String,
    pub address: String,
    pub phone: String,
    pub email: String,
    pub logo_path: Option<String>,
    pub default_tax_basis_points: i64,
    pub default_turnaround_days: i64,
    pub quote_prefix: String,
    pub invoice_prefix: String,
    pub next_quote_number: i64,
    pub next_invoice_number: i64,
}

#[derive(Clone, Debug, FromRow)]
pub struct Document {
    pub id: String,
    pub number: String,
    pub status: String,
    pub customer_name: String,
    pub customer_email: String,
    pub customer_phone: String,
    pub billing_address: String,
    pub issue_date: String,
    pub expiry_or_due_date: Option<String>,
    pub discount_cents: i64,
    pub tax_basis_points: i64,
    pub notes: String,
    pub terms: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, FromRow)]
pub struct DocumentLine {
    pub description: String,
    pub quantity: String,
    pub unit_label: String,
    pub unit_price_cents: i64,
}

#[derive(Clone, Debug)]
pub struct FullDocument {
    pub document: Document,
    pub lines: Vec<DocumentLine>,
}

impl FullDocument {
    pub fn total_cents(&self) -> i64 {
        calculate_document(
            &self
                .lines
                .iter()
                .filter_map(|line| {
                    Some(LineCalculationInput {
                        quantity: line.quantity.parse().ok()?,
                        unit_price: Money::from_cents(line.unit_price_cents).ok()?,
                    })
                })
                .collect::<Vec<_>>(),
            Money::from_cents(self.document.discount_cents).unwrap_or(Money::ZERO),
            u32::try_from(self.document.tax_basis_points).unwrap_or(0),
        )
        .map_or(0, |totals| totals.total.cents())
    }
    pub fn to_input(&self) -> DocumentInput {
        DocumentInput {
            status: self.document.status.clone(),
            customer_name: self.document.customer_name.clone(),
            customer_email: self.document.customer_email.clone(),
            customer_phone: self.document.customer_phone.clone(),
            billing_address: self.document.billing_address.clone(),
            issue_date: self.document.issue_date.clone(),
            expiry_or_due_date: self.document.expiry_or_due_date.clone().unwrap_or_default(),
            discount: format_cents(self.document.discount_cents),
            tax_rate: format_rate(self.document.tax_basis_points),
            notes: self.document.notes.clone(),
            terms: self.document.terms.clone(),
            lines: self
                .lines
                .iter()
                .map(|line| LineInput {
                    description: line.description.clone(),
                    quantity: line.quantity.clone(),
                    unit_label: line.unit_label.clone(),
                    unit_price: format_cents(line.unit_price_cents),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, FromRow)]
pub struct Job {
    pub id: String,
    pub title: String,
    pub description: String,
    pub customer_name: String,
    pub customer_phone: String,
    pub customer_email: String,
    pub category: String,
    pub material: String,
    pub status: String,
    pub due_date: Option<String>,
    pub quote_id: Option<String>,
    pub invoice_id: Option<String>,
    pub picked_up_at: Option<String>,
    pub completed_at: Option<String>,
    pub invoice_status: Option<String>,
    pub notes: String,
    pub price_cents: i64,
}

#[derive(Clone, Debug, FromRow, Eq, PartialEq)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color_key: String,
    pub archived_at: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, FromRow, Eq, PartialEq)]
pub struct JobAttachment {
    pub id: String,
    pub job_id: String,
    pub original_name: String,
    pub stored_name: String,
    pub media_type: String,
    pub byte_size: i64,
    pub sha256: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewJobAttachment<'a> {
    pub original_name: &'a str,
    pub stored_name: &'a str,
    pub media_type: &'a str,
    pub byte_size: i64,
    pub sha256: &'a str,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Clone, Debug, FromRow)]
pub struct QuoteWorkItem {
    pub id: String,
    pub number: String,
    pub status: String,
    pub customer_name: String,
}

#[derive(Clone, Debug, Default)]
pub struct JobFilter {
    pub search: String,
    pub category: String,
    pub status: String,
    pub material: String,
    pub billing: String,
}

#[derive(Clone, Debug, Default)]
pub struct ArchiveFilter {
    pub search: String,
    pub invoice: String,
    pub pickup: String,
    pub completed_from: String,
    pub completed_through: String,
    pub category: String,
    pub tag_id: String,
}

#[derive(Clone, Debug, FromRow, Eq, PartialEq)]
pub struct JobTagProjection {
    pub job_id: String,
    pub id: String,
    pub name: String,
    pub color_key: String,
}

#[derive(Clone, Debug, FromRow, Eq, PartialEq)]
pub struct JobAttachmentCount {
    pub job_id: String,
    pub attachment_count: i64,
}

#[derive(Clone, Debug, FromRow, Eq, PartialEq)]
pub struct JobAttachmentProjection {
    pub id: String,
    pub job_id: String,
    pub original_name: String,
    pub media_type: String,
    pub byte_size: i64,
}

#[derive(Clone, Debug, FromRow, Eq, PartialEq)]
pub struct JobEvent {
    pub kind: String,
    pub summary: String,
    pub occurred_at: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JobCounts {
    pub total: usize,
    pub active: usize,
}

#[derive(Clone)]
pub struct Repository {
    pub(super) pool: SqlitePool,
    pub(super) shop_time_zone: Tz,
}

impl Repository {
    #[cfg(test)]
    pub fn new(pool: SqlitePool) -> Self {
        Self::with_shop_time_zone(pool, crate::config::DEFAULT_SHOP_TIME_ZONE)
    }

    pub fn with_shop_time_zone(pool: SqlitePool, shop_time_zone: Tz) -> Self {
        Self {
            pool,
            shop_time_zone,
        }
    }

    pub(crate) fn business_today(&self) -> NaiveDate {
        business_date_at(Utc::now(), self.shop_time_zone)
    }
}

fn business_date_at(instant: DateTime<Utc>, shop_time_zone: Tz) -> NaiveDate {
    instant.with_timezone(&shop_time_zone).date_naive()
}

#[cfg(test)]
mod tests;
