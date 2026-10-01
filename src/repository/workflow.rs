use chrono::Utc;
use sqlx::{FromRow, SqliteConnection};
use uuid::Uuid;

use super::{Document, Repository, Settings, events::record_job_event};

#[derive(FromRow)]
struct DirectJobInvoiceSource {
    title: String,
    customer_name: String,
    customer_phone: String,
    customer_email: String,
    quote_id: Option<String>,
    invoice_id: Option<String>,
}

impl Repository {
    pub async fn copy_quote_to_invoice(
        &self,
        quote_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // Acquire SQLite's write lock before reading the conversion links. This keeps
        // concurrent quote/job actions from both deciding that they need a new invoice.
        sqlx::query(
            "UPDATE settings SET next_invoice_number=next_invoice_number WHERE singleton=1",
        )
        .execute(&mut *tx)
        .await?;

        if let Some(invoice_id) = sqlx::query_scalar::<_, String>(
            "SELECT id FROM documents WHERE kind='invoice' AND source_quote_id=?",
        )
        .bind(quote_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            let now = Utc::now().to_rfc3339();
            link_quote_job_to_invoice(&mut tx, quote_id, &invoice_id, false, &now).await?;
            tx.commit().await?;
            return Ok(Some(invoice_id));
        }

        if let Some(invoice_id) = sqlx::query_scalar::<_, String>(
            "SELECT invoice_id FROM jobs WHERE quote_id=? AND invoice_id IS NOT NULL",
        )
        .bind(quote_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            sqlx::query(
                "UPDATE documents SET source_quote_id=? WHERE id=? AND kind='invoice' AND source_quote_id IS NULL",
            )
            .bind(quote_id)
            .bind(&invoice_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(Some(invoice_id));
        }

        let quote: Option<Document> = sqlx::query_as("SELECT id,number,status,customer_name,customer_email,customer_phone,billing_address,issue_date,expiry_or_due_date,discount_cents,tax_basis_points,notes,terms,updated_at FROM documents WHERE id=? AND kind='quote'")
            .bind(quote_id)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(quote) = quote else {
            tx.rollback().await?;
            return Ok(None);
        };

        let settings: Settings = sqlx::query_as("SELECT business_name,address,phone,email,logo_path,default_tax_basis_points,default_turnaround_days,quote_prefix,invoice_prefix,next_quote_number,next_invoice_number FROM settings WHERE singleton=1")
            .fetch_one(&mut *tx)
            .await?;
        let invoice_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let number = format!(
            "{}{}",
            settings.invoice_prefix, settings.next_invoice_number
        );
        sqlx::query("INSERT INTO documents (id,kind,number,status,customer_name,customer_email,customer_phone,billing_address,issue_date,expiry_or_due_date,discount_cents,tax_basis_points,notes,terms,last_emailed_at,resend_message_id,source_quote_id,created_at,updated_at) VALUES (?,'invoice',?,'draft',?,?,?,?,?,NULL,?,?,?,?,NULL,NULL,?,?,?)")
            .bind(&invoice_id)
            .bind(number)
            .bind(&quote.customer_name)
            .bind(&quote.customer_email)
            .bind(&quote.customer_phone)
            .bind(&quote.billing_address)
            .bind(&quote.issue_date)
            .bind(quote.discount_cents)
            .bind(quote.tax_basis_points)
            .bind(&quote.notes)
            .bind(&quote.terms)
            .bind(quote_id)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE settings SET next_invoice_number=next_invoice_number+1 WHERE singleton=1",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO document_lines (id,document_id,display_position,description,quantity,unit_label,unit_price_cents) SELECT lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))),2) || '-' || substr('89ab',abs(random()) % 4 + 1,1) || substr(lower(hex(randomblob(2))),2) || '-' || lower(hex(randomblob(6))), ?, display_position, description, quantity, unit_label, unit_price_cents FROM document_lines WHERE document_id=?")
            .bind(&invoice_id)
            .bind(quote_id)
            .execute(&mut *tx)
            .await?;
        link_quote_job_to_invoice(&mut tx, quote_id, &invoice_id, true, &now).await?;
        tx.commit().await?;
        Ok(Some(invoice_id))
    }
    #[allow(dead_code)]
    pub async fn start_job_from_quote(
        &self,
        quote_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = self.start_quoted_job_on(&mut tx, quote_id).await?;
        if result.is_some() {
            tx.commit().await?;
        } else {
            tx.rollback().await?;
        }
        Ok(result)
    }

    pub(super) async fn start_quoted_job_on(
        &self,
        connection: &mut SqliteConnection,
        quote_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        Self::start_job_from_quote_on(connection, quote_id, self.business_today()).await
    }

    async fn start_job_from_quote_on(
        connection: &mut SqliteConnection,
        quote_id: &str,
        business_date: chrono::NaiveDate,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query(
            "UPDATE settings SET next_invoice_number=next_invoice_number WHERE singleton=1",
        )
        .execute(&mut *connection)
        .await?;
        if let Some(job_id) =
            sqlx::query_scalar::<_, String>("SELECT id FROM jobs WHERE quote_id=?")
                .bind(quote_id)
                .fetch_optional(&mut *connection)
                .await?
        {
            let now = Utc::now().to_rfc3339();
            let linked = sqlx::query(
                "UPDATE jobs SET invoice_id=(SELECT id FROM documents WHERE kind='invoice' AND source_quote_id=?),updated_at=? WHERE id=? AND invoice_id IS NULL AND EXISTS (SELECT 1 FROM documents WHERE kind='invoice' AND source_quote_id=?)",
            )
            .bind(quote_id)
            .bind(&now)
            .bind(&job_id)
            .bind(quote_id)
            .execute(&mut *connection)
            .await?;
            if linked.rows_affected() == 1 {
                let invoice_id: String =
                    sqlx::query_scalar("SELECT invoice_id FROM jobs WHERE id=?")
                        .bind(&job_id)
                        .fetch_one(&mut *connection)
                        .await?;
                record_invoice_event(connection, &job_id, &invoice_id, false, &now).await?;
            }
            return Ok(Some(job_id));
        }
        let quote: Option<(String, String, String, String)> = sqlx::query_as(
            "SELECT number,customer_name,customer_phone,customer_email FROM documents WHERE id=? AND kind='quote' AND status <> 'declined'",
        )
        .bind(quote_id)
        .fetch_optional(&mut *connection)
        .await?;
        let Some((number, customer_name, customer_phone, customer_email)) = quote else {
            return Ok(None);
        };
        let job_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let inserted = sqlx::query("INSERT INTO jobs (id,title,customer_name,customer_phone,customer_email,category,status,due_date,quote_id,invoice_id,notes,created_at,updated_at) VALUES (?,?,?,?,?,'Quoted work','design',date(?,'+' || (SELECT default_turnaround_days FROM settings WHERE singleton=1) || ' days'),?,(SELECT id FROM documents WHERE kind='invoice' AND source_quote_id=?),'',?,?) ON CONFLICT(quote_id) DO NOTHING")
            .bind(&job_id)
            .bind(format!("Quote {number}"))
            .bind(customer_name)
            .bind(customer_phone)
            .bind(customer_email)
            .bind(business_date.to_string())
            .bind(quote_id)
            .bind(quote_id)
            .bind(&now)
            .bind(&now)
            .execute(&mut *connection)
            .await?;
        let linked_id = sqlx::query_scalar::<_, String>("SELECT id FROM jobs WHERE quote_id=?")
            .bind(quote_id)
            .fetch_one(&mut *connection)
            .await?;
        if inserted.rows_affected() == 1 {
            record_job_event(connection, &linked_id, "created", "Job created", &now).await?;
            if let Some(invoice_id) = sqlx::query_scalar::<_, String>(
                "SELECT invoice_id FROM jobs WHERE id=? AND invoice_id IS NOT NULL",
            )
            .bind(&linked_id)
            .fetch_optional(&mut *connection)
            .await?
            {
                record_invoice_event(connection, &linked_id, &invoice_id, false, &now).await?;
            }
        }
        Ok(Some(linked_id))
    }

    pub async fn create_or_open_job_invoice(
        &self,
        job_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE settings SET next_invoice_number=next_invoice_number WHERE singleton=1",
        )
        .execute(&mut *tx)
        .await?;
        let job: Option<DirectJobInvoiceSource> =
            sqlx::query_as("SELECT title,customer_name,customer_phone,customer_email,quote_id,invoice_id FROM jobs WHERE id=?")
                .bind(job_id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(job) = job else {
            tx.rollback().await?;
            return Ok(None);
        };
        if let Some(invoice_id) = job.invoice_id {
            tx.commit().await?;
            return Ok(Some(invoice_id));
        }
        if let Some(quote_id) = job.quote_id {
            tx.rollback().await?;
            return self.copy_quote_to_invoice(&quote_id).await;
        }

        let settings: Settings = sqlx::query_as("SELECT business_name,address,phone,email,logo_path,default_tax_basis_points,default_turnaround_days,quote_prefix,invoice_prefix,next_quote_number,next_invoice_number FROM settings WHERE singleton=1")
            .fetch_one(&mut *tx)
            .await?;
        let invoice_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let number = format!(
            "{}{}",
            settings.invoice_prefix, settings.next_invoice_number
        );
        sqlx::query("INSERT INTO documents (id,kind,number,status,customer_name,customer_email,customer_phone,billing_address,issue_date,discount_cents,tax_basis_points,notes,terms,created_at,updated_at) VALUES (?,'invoice',?,'draft',?,?,?,'',?,0,?,'','',?,?)")
            .bind(&invoice_id)
            .bind(number)
            .bind(job.customer_name)
            .bind(job.customer_email)
            .bind(job.customer_phone)
            .bind(self.business_today().to_string())
            .bind(settings.default_tax_basis_points)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO document_lines (id,document_id,display_position,description,quantity,unit_label,unit_price_cents) VALUES (?,?,0,?,'1','job',0)")
            .bind(Uuid::new_v4().to_string())
            .bind(&invoice_id)
            .bind(job.title)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE settings SET next_invoice_number=next_invoice_number+1 WHERE singleton=1",
        )
        .execute(&mut *tx)
        .await?;
        let linked = sqlx::query(
            "UPDATE jobs SET invoice_id=?,updated_at=? WHERE id=? AND invoice_id IS NULL",
        )
        .bind(&invoice_id)
        .bind(&now)
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
        if linked.rows_affected() != 1 {
            tx.rollback().await?;
            return sqlx::query_scalar::<_, String>("SELECT invoice_id FROM jobs WHERE id=?")
                .bind(job_id)
                .fetch_optional(&self.pool)
                .await;
        }
        record_invoice_event(&mut tx, job_id, &invoice_id, true, &now).await?;
        tx.commit().await?;
        Ok(Some(invoice_id))
    }
    pub async fn mark_paid(&self, id: &str) -> Result<bool, sqlx::Error> {
        Ok(sqlx::query(
            "UPDATE documents SET status='paid',updated_at=? WHERE id=? AND kind='invoice'",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected()
            == 1)
    }

    pub async fn accept_quote(&self, id: &str) -> Result<bool, sqlx::Error> {
        Ok(sqlx::query(
            "UPDATE documents SET status='accepted',updated_at=? WHERE id=? AND kind='quote' AND status <> 'declined'",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected()
            == 1)
    }

    pub async fn decline_quote(&self, id: &str) -> Result<bool, sqlx::Error> {
        Ok(sqlx::query(
            "UPDATE documents SET status='declined',updated_at=? WHERE id=? AND kind='quote'",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected()
            == 1)
    }
}

async fn record_invoice_event(
    connection: &mut SqliteConnection,
    job_id: &str,
    invoice_id: &str,
    created: bool,
    occurred_at: &str,
) -> Result<(), sqlx::Error> {
    let number: String =
        sqlx::query_scalar("SELECT number FROM documents WHERE id=? AND kind='invoice'")
            .bind(invoice_id)
            .fetch_one(&mut *connection)
            .await?;
    let summary = if created {
        format!("Invoice {number} created and linked")
    } else {
        format!("Invoice {number} linked")
    };
    let kind = if created {
        "invoice_created"
    } else {
        "invoice_linked"
    };
    record_job_event(connection, job_id, kind, &summary, occurred_at).await
}

async fn link_quote_job_to_invoice(
    connection: &mut SqliteConnection,
    quote_id: &str,
    invoice_id: &str,
    created: bool,
    occurred_at: &str,
) -> Result<(), sqlx::Error> {
    let linked = sqlx::query(
        "UPDATE jobs SET invoice_id=?,updated_at=? WHERE quote_id=? AND invoice_id IS NULL",
    )
    .bind(invoice_id)
    .bind(occurred_at)
    .bind(quote_id)
    .execute(&mut *connection)
    .await?;
    if linked.rows_affected() == 1
        && let Some(job_id) =
            sqlx::query_scalar::<_, String>("SELECT id FROM jobs WHERE quote_id=? AND invoice_id=?")
                .bind(quote_id)
                .bind(invoice_id)
                .fetch_optional(&mut *connection)
                .await?
    {
        record_invoice_event(connection, &job_id, invoice_id, created, occurred_at).await?;
    }
    Ok(())
}
