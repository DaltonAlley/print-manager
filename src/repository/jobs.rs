use std::str::FromStr;

use chrono::{Days, Utc};
use sqlx::{FromRow, QueryBuilder, Sqlite};
use uuid::Uuid;

use crate::domain::{JobCardUpdateInput, JobInput, JobStage, parse_money};

use super::{ArchiveFilter, Job, JobCounts, JobFilter, Repository, events::record_job_event};

#[derive(FromRow)]
struct EditableJobFields {
    title: String,
    description: String,
    customer_name: String,
    customer_phone: String,
    customer_email: String,
    category: String,
    material: String,
    due_date: Option<String>,
    notes: String,
    price_cents: i64,
}

impl Repository {
    pub async fn job_counts(&self) -> Result<JobCounts, sqlx::Error> {
        let (total, active): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(CASE WHEN jobs.completed_at IS NULL THEN 1 ELSE 0 END), 0) FROM jobs",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(JobCounts {
            total: usize::try_from(total).unwrap_or(usize::MAX),
            active: usize::try_from(active).unwrap_or(usize::MAX),
        })
    }
    pub async fn categories(&self) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT DISTINCT category FROM jobs WHERE category <> '' ORDER BY category",
        )
        .fetch_all(&self.pool)
        .await
    }
    pub async fn materials(&self) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT DISTINCT material FROM jobs WHERE material <> '' ORDER BY material COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await
    }
    pub async fn job(&self, id: &str) -> Result<Option<Job>, sqlx::Error> {
        sqlx::query_as("SELECT jobs.id,jobs.title,jobs.description,jobs.customer_name,jobs.customer_phone,jobs.customer_email,jobs.category,jobs.material,jobs.status,jobs.due_date,jobs.quote_id,jobs.invoice_id,jobs.picked_up_at,jobs.completed_at,invoices.status AS invoice_status,jobs.notes,jobs.price_cents FROM jobs LEFT JOIN documents AS invoices ON invoices.id=jobs.invoice_id AND invoices.kind='invoice' WHERE jobs.id=?").bind(id).fetch_optional(&self.pool).await
    }

    pub async fn board_jobs(&self, filter: &JobFilter) -> Result<Vec<Job>, sqlx::Error> {
        let mut query = QueryBuilder::<Sqlite>::new(
            "WITH ranked_completed AS (SELECT id,ROW_NUMBER() OVER (ORDER BY completed_at DESC,id DESC) AS completion_rank FROM jobs WHERE completed_at IS NOT NULL), board_jobs AS (SELECT jobs.*,0 AS phase_rank FROM jobs WHERE jobs.completed_at IS NULL UNION ALL SELECT jobs.*,1 AS phase_rank FROM jobs JOIN ranked_completed ON ranked_completed.id=jobs.id WHERE ranked_completed.completion_rank <= 10) SELECT board_jobs.id,board_jobs.title,board_jobs.description,board_jobs.customer_name,board_jobs.customer_phone,board_jobs.customer_email,board_jobs.category,board_jobs.material,board_jobs.status,board_jobs.due_date,board_jobs.quote_id,board_jobs.invoice_id,board_jobs.picked_up_at,board_jobs.completed_at,invoices.status AS invoice_status,board_jobs.notes,board_jobs.price_cents FROM board_jobs LEFT JOIN documents AS invoices ON invoices.id=board_jobs.invoice_id AND invoices.kind='invoice' WHERE 1=1",
        );
        if !filter.search.is_empty() {
            let search = format!("%{}%", filter.search.trim());
            query
                .push(" AND (board_jobs.title LIKE ")
                .push_bind(search.clone())
                .push(" OR board_jobs.customer_name LIKE ")
                .push_bind(search.clone())
                .push(" OR board_jobs.category LIKE ")
                .push_bind(search.clone())
                .push(" OR board_jobs.material LIKE ")
                .push_bind(search)
                .push(")");
        }
        if !filter.category.is_empty() {
            query
                .push(" AND board_jobs.category = ")
                .push_bind(&filter.category);
        }
        if !filter.status.is_empty() {
            query
                .push(" AND (board_jobs.completed_at IS NOT NULL OR board_jobs.status = ")
                .push_bind(&filter.status)
                .push(")");
        }
        if !filter.material.is_empty() {
            query
                .push(" AND board_jobs.material = ")
                .push_bind(filter.material.trim().to_owned());
        }
        match filter.billing.as_str() {
            "paid" => query.push(" AND invoices.status = 'paid'"),
            "unpaid" => query.push(" AND board_jobs.invoice_id IS NOT NULL AND (invoices.status IS NULL OR invoices.status <> 'paid')"),
            "none" => query.push(" AND board_jobs.invoice_id IS NULL"),
            _ => &mut query,
        };
        query.push(" ORDER BY CASE board_jobs.status WHEN 'design' THEN 0 WHEN 'waiting' THEN 1 WHEN 'print' THEN 2 WHEN 'finish' THEN 3 WHEN 'complete' THEN 4 ELSE 5 END,CASE board_jobs.status WHEN 'design' THEN board_jobs.design_board_position WHEN 'waiting' THEN board_jobs.waiting_board_position WHEN 'print' THEN board_jobs.print_board_position WHEN 'finish' THEN board_jobs.finish_board_position WHEN 'complete' THEN board_jobs.complete_board_position END IS NULL,CASE board_jobs.status WHEN 'design' THEN board_jobs.design_board_position WHEN 'waiting' THEN board_jobs.waiting_board_position WHEN 'print' THEN board_jobs.print_board_position WHEN 'finish' THEN board_jobs.finish_board_position WHEN 'complete' THEN board_jobs.complete_board_position END,board_jobs.due_date IS NULL,board_jobs.due_date,board_jobs.completed_at DESC,board_jobs.updated_at DESC,board_jobs.id DESC");
        query.build_query_as().fetch_all(&self.pool).await
    }

    /// Changes the scheduling date for an active job and records the edit atomically.
    pub async fn schedule_job(
        &self,
        id: &str,
        due_date: Option<&str>,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let current: Option<Option<String>> =
            sqlx::query_scalar("SELECT due_date FROM jobs WHERE id=? AND completed_at IS NULL")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(current) = current else {
            tx.rollback().await?;
            return Ok(false);
        };
        if current.as_deref() == due_date {
            tx.rollback().await?;
            return Ok(false);
        }
        let now = Utc::now().to_rfc3339();
        let updated = sqlx::query(
            "UPDATE jobs SET due_date=?,updated_at=? WHERE id=? AND completed_at IS NULL",
        )
        .bind(due_date)
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(false);
        }
        let summary = due_date.map_or_else(
            || "Due date cleared".to_owned(),
            |date| format!("Due date changed to {date}"),
        );
        record_job_event(&mut tx, id, "details_edited", &summary, &now).await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn filtered_archived_jobs(
        &self,
        filter: &ArchiveFilter,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Job>, sqlx::Error> {
        let limit = limit.clamp(1, 100);
        let mut query = QueryBuilder::<Sqlite>::new(
            "WITH ranked_jobs AS (SELECT jobs.*,ROW_NUMBER() OVER (ORDER BY jobs.completed_at DESC,jobs.id DESC) AS completion_rank FROM jobs WHERE jobs.completed_at IS NOT NULL) SELECT ranked_jobs.id,ranked_jobs.title,ranked_jobs.description,ranked_jobs.customer_name,ranked_jobs.customer_phone,ranked_jobs.customer_email,ranked_jobs.category,ranked_jobs.material,ranked_jobs.status,ranked_jobs.due_date,ranked_jobs.quote_id,ranked_jobs.invoice_id,ranked_jobs.picked_up_at,ranked_jobs.completed_at,invoices.status AS invoice_status,ranked_jobs.notes,ranked_jobs.price_cents FROM ranked_jobs LEFT JOIN documents AS invoices ON invoices.id=ranked_jobs.invoice_id AND invoices.kind='invoice' WHERE ranked_jobs.completion_rank > 10",
        );
        push_archive_filters(&mut query, filter);
        query
            .push(" ORDER BY ranked_jobs.completion_rank LIMIT ")
            .push_bind(i64::from(limit))
            .push(" OFFSET ")
            .push_bind(i64::from(offset));
        query.build_query_as().fetch_all(&self.pool).await
    }

    pub async fn archived_job_count(&self, filter: &ArchiveFilter) -> Result<usize, sqlx::Error> {
        let mut query = QueryBuilder::<Sqlite>::new(
            "WITH ranked_jobs AS (SELECT jobs.*,ROW_NUMBER() OVER (ORDER BY jobs.completed_at DESC,jobs.id DESC) AS completion_rank FROM jobs WHERE jobs.completed_at IS NOT NULL) SELECT COUNT(*) FROM ranked_jobs LEFT JOIN documents AS invoices ON invoices.id=ranked_jobs.invoice_id AND invoices.kind='invoice' WHERE ranked_jobs.completion_rank > 10",
        );
        push_archive_filters(&mut query, filter);
        let count: i64 = query.build_query_scalar().fetch_one(&self.pool).await?;
        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }

    pub async fn save_job(
        &self,
        id: Option<&str>,
        input: &JobInput,
    ) -> Result<String, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        if id.is_none()
            && let Some(quote_id) = input.quote_id.as_deref().and_then(option)
            && let Some(existing) =
                sqlx::query_scalar::<_, String>("SELECT id FROM jobs WHERE quote_id=?")
                    .bind(quote_id)
                    .fetch_optional(&mut *tx)
                    .await?
        {
            tx.commit().await?;
            return Ok(existing);
        }
        let generated_id = id.is_none();
        let id = id.map_or_else(|| Uuid::new_v4().to_string(), ToOwned::to_owned);
        let previous: Option<EditableJobFields> = if generated_id {
            None
        } else {
            sqlx::query_as(
                "SELECT title,description,customer_name,customer_phone,customer_email,category,material,due_date,notes,price_cents FROM jobs WHERE id=?",
            )
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await?
        };
        let existed = previous.is_some();
        let now = Utc::now().to_rfc3339();
        let existing_due_date = previous.as_ref().and_then(|job| job.due_date.clone());
        let due_date = if let Some(due_date) = option(&input.due_date) {
            Some(due_date.to_owned())
        } else if existing_due_date.is_some() {
            existing_due_date
        } else {
            let turnaround_days: i64 = sqlx::query_scalar(
                "SELECT default_turnaround_days FROM settings WHERE singleton=1",
            )
            .fetch_one(&mut *tx)
            .await?;
            let days = u64::try_from(turnaround_days).unwrap_or(3);
            self.business_today()
                .checked_add_days(Days::new(days))
                .map(|date| date.to_string())
        };
        let price_cents = parse_money(&input.price).unwrap_or_default();
        let result = sqlx::query("INSERT INTO jobs (id,title,description,customer_name,customer_phone,customer_email,category,material,status,due_date,quote_id,notes,price_cents,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET title=excluded.title,description=excluded.description,customer_name=excluded.customer_name,customer_phone=excluded.customer_phone,customer_email=excluded.customer_email,category=excluded.category,material=excluded.material,due_date=excluded.due_date,notes=excluded.notes,price_cents=excluded.price_cents,updated_at=excluded.updated_at ON CONFLICT(quote_id) DO NOTHING")
            .bind(&id)
            .bind(input.title.trim())
            .bind(input.description.trim())
            .bind(&input.customer_name)
            .bind(&input.customer_phone)
            .bind(&input.customer_email)
            .bind(input.category.trim())
            .bind(input.material.trim())
            .bind(&input.status)
            .bind(due_date.as_deref())
            .bind(input.quote_id.as_deref().and_then(option))
            .bind(input.notes.trim())
            .bind(price_cents)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0
            && let Some(quote_id) = input.quote_id.as_deref().and_then(option)
            && let Some(existing) =
                sqlx::query_scalar::<_, String>("SELECT id FROM jobs WHERE quote_id=?")
                    .bind(quote_id)
                    .fetch_optional(&mut *tx)
                    .await?
        {
            tx.commit().await?;
            return Ok(existing);
        }
        if !existed && result.rows_affected() == 1 {
            record_job_event(&mut tx, &id, "created", "Job created", &now).await?;
        } else if let Some(previous) = previous {
            let changed = changed_job_fields(&previous, input, due_date.as_deref(), price_cents);
            if !changed.is_empty() {
                record_job_event(
                    &mut tx,
                    &id,
                    "details_edited",
                    &format!("Job details updated: {}", changed.join(", ")),
                    &now,
                )
                .await?;
            }
        }
        tx.commit().await?;
        Ok(id)
    }

    pub async fn update_job_card(
        &self,
        id: &str,
        input: &JobCardUpdateInput,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let previous: Option<(String, String, String, String)> = sqlx::query_as(
            "SELECT customer_name,customer_phone,customer_email,notes FROM jobs WHERE id=? AND completed_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(previous) = previous else {
            tx.commit().await?;
            return Ok(false);
        };
        let values = (
            input.customer_name.as_str(),
            input.customer_phone.as_str(),
            input.customer_email.as_str(),
            input.notes.trim(),
        );
        sqlx::query("UPDATE jobs SET customer_name=?,customer_phone=?,customer_email=?,notes=?,updated_at=? WHERE id=? AND completed_at IS NULL")
            .bind(values.0)
            .bind(values.1)
            .bind(values.2)
            .bind(values.3)
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let changed = [
            (previous.0 != values.0, "customer"),
            (previous.1 != values.1, "customer phone"),
            (previous.2 != values.2, "customer email"),
            (previous.3 != values.3, "notes"),
        ]
        .into_iter()
        .filter_map(|(changed, field)| changed.then_some(field))
        .collect::<Vec<_>>();
        if !changed.is_empty() {
            record_job_event(
                &mut tx,
                id,
                "details_edited",
                &format!("Job details updated: {}", changed.join(", ")),
                &Utc::now().to_rfc3339(),
            )
            .await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    pub async fn add_job_comment(&self, id: &str, comment: &str) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let active: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM jobs WHERE id=? AND completed_at IS NULL")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        if active.is_none() {
            tx.commit().await?;
            return Ok(false);
        }
        record_job_event(
            &mut tx,
            id,
            "comment",
            comment.trim(),
            &Utc::now().to_rfc3339(),
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn set_job_status(&self, id: &str, status: &str) -> Result<bool, sqlx::Error> {
        match JobStage::from_str(status) {
            Ok(JobStage::Complete) => self.complete_job(id).await,
            Ok(_) => self.reopen_job(id, status).await,
            Err(_) => Ok(false),
        }
    }
    pub async fn complete_job(&self, id: &str) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now().to_rfc3339();
        let previous: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT status,completed_at FROM jobs WHERE id=? AND picked_up_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let updated = sqlx::query("UPDATE jobs SET status='complete',completed_at=COALESCE(completed_at,?),updated_at=? WHERE id=? AND picked_up_at IS NULL")
            .bind(&now)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if updated.rows_affected() == 1
            && previous.is_some_and(|(status, completed_at)| {
                status != "complete" || completed_at.is_none()
            })
        {
            record_job_event(&mut tx, id, "completed", "Job completed", &now).await?;
        }
        tx.commit().await?;
        Ok(updated.rows_affected() == 1)
    }
    pub async fn reopen_job(&self, id: &str, stage: &str) -> Result<bool, sqlx::Error> {
        let Ok(stage) = JobStage::from_str(stage) else {
            return Ok(false);
        };
        if stage == JobStage::Complete {
            return Ok(false);
        }
        let mut tx = self.pool.begin().await?;
        let previous: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT status,completed_at FROM jobs WHERE id=? AND picked_up_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let now = Utc::now().to_rfc3339();
        let updated = sqlx::query("UPDATE jobs SET status=?,completed_at=NULL,updated_at=? WHERE id=? AND picked_up_at IS NULL")
            .bind(stage.as_str())
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if updated.rows_affected() == 1
            && let Some((old_stage, completed_at)) = previous
            && (old_stage != stage.as_str() || completed_at.is_some())
        {
            let (kind, summary) = if completed_at.is_some() {
                (
                    "reopened",
                    format!("Job reopened in {}", stage_label(stage)),
                )
            } else {
                (
                    "stage_changed",
                    format!(
                        "Moved from {} to {}",
                        stage_text(&old_stage),
                        stage_label(stage)
                    ),
                )
            };
            record_job_event(&mut tx, id, kind, &summary, &now).await?;
        }
        tx.commit().await?;
        Ok(updated.rows_affected() == 1)
    }
    pub async fn set_job_picked_up(&self, id: &str, picked_up: bool) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now().to_rfc3339();
        let picked_up_at = picked_up.then_some(now.as_str());
        let previous: Option<Option<String>> =
            sqlx::query_scalar("SELECT picked_up_at FROM jobs WHERE id=?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let updated = sqlx::query("UPDATE jobs SET picked_up_at=?,updated_at=? WHERE id=? AND (? IS NULL OR status='complete')")
                .bind(picked_up_at)
                .bind(&now)
                .bind(id)
                .bind(picked_up_at)
                .execute(&mut *tx)
                .await?;
        if updated.rows_affected() == 1 && previous.flatten().is_some() != picked_up {
            let (kind, summary) = if picked_up {
                ("pickup_confirmed", "Pickup confirmed")
            } else {
                ("pickup_reopened", "Pickup confirmation removed")
            };
            record_job_event(&mut tx, id, kind, summary, &now).await?;
        }
        tx.commit().await?;
        Ok(updated.rows_affected() == 1)
    }
}

fn changed_job_fields(
    previous: &EditableJobFields,
    input: &JobInput,
    due_date: Option<&str>,
    price_cents: i64,
) -> Vec<&'static str> {
    let mut changed = Vec::new();
    for (different, label) in [
        (previous.title != input.title.trim(), "title"),
        (
            previous.description != input.description.trim(),
            "description",
        ),
        (previous.customer_name != input.customer_name, "customer"),
        (
            previous.customer_phone != input.customer_phone,
            "customer phone",
        ),
        (
            previous.customer_email != input.customer_email,
            "customer email",
        ),
        (previous.category != input.category.trim(), "category"),
        (previous.material != input.material.trim(), "material"),
        (previous.due_date.as_deref() != due_date, "due date"),
        (previous.notes != input.notes.trim(), "notes"),
        (previous.price_cents != price_cents, "price"),
    ] {
        if different {
            changed.push(label);
        }
    }
    changed
}

fn stage_label(stage: JobStage) -> &'static str {
    match stage {
        JobStage::Design => "Design",
        JobStage::Waiting => "Waiting",
        JobStage::Print => "Print",
        JobStage::Finish => "Finish",
        JobStage::Complete => "Complete",
    }
}

fn stage_text(stage: &str) -> &str {
    match JobStage::from_str(stage) {
        Ok(stage) => stage_label(stage),
        Err(_) => stage,
    }
}

fn push_archive_filters(query: &mut QueryBuilder<'_, Sqlite>, filter: &ArchiveFilter) {
    if !filter.search.is_empty() {
        let search = format!("%{}%", filter.search.trim());
        query
            .push(" AND (ranked_jobs.title LIKE ")
            .push_bind(search.clone())
            .push(" OR ranked_jobs.customer_name LIKE ")
            .push_bind(search)
            .push(")");
    }
    match filter.invoice.as_str() {
        "paid" => {
            query.push(" AND invoices.status='paid'");
        }
        "unpaid" => {
            query.push(" AND ranked_jobs.invoice_id IS NOT NULL AND COALESCE(invoices.status,'') <> 'paid'");
        }
        "none" => {
            query.push(" AND ranked_jobs.invoice_id IS NULL");
        }
        _ => {}
    }
    match filter.pickup.as_str() {
        "picked-up" => {
            query.push(" AND ranked_jobs.picked_up_at IS NOT NULL");
        }
        "awaiting" => {
            query.push(" AND ranked_jobs.picked_up_at IS NULL");
        }
        _ => {}
    }
    if !filter.completed_from.is_empty() {
        query
            .push(" AND date(ranked_jobs.completed_at) >= date(")
            .push_bind(filter.completed_from.trim().to_owned())
            .push(")");
    }
    if !filter.completed_through.is_empty() {
        query
            .push(" AND date(ranked_jobs.completed_at) <= date(")
            .push_bind(filter.completed_through.trim().to_owned())
            .push(")");
    }
    if !filter.category.is_empty() {
        query
            .push(" AND ranked_jobs.category = ")
            .push_bind(filter.category.trim().to_owned());
    }
    if !filter.tag_id.is_empty() {
        query
            .push(" AND EXISTS (SELECT 1 FROM job_tags WHERE job_tags.job_id=ranked_jobs.id AND job_tags.tag_id=")
            .push_bind(filter.tag_id.trim().to_owned())
            .push(")");
    }
}

fn option(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value.trim())
}
