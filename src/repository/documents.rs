use chrono::Utc;
use sqlx::Sqlite;
use uuid::Uuid;

use crate::domain::{DocumentKind, ValidDocument};

use super::{Document, DocumentLine, FullDocument, Repository, Settings};

impl Repository {
    pub async fn documents(&self, kind: DocumentKind) -> Result<Vec<FullDocument>, sqlx::Error> {
        let docs: Vec<Document> = sqlx::query_as("SELECT id,number,status,customer_name,customer_email,customer_phone,billing_address,issue_date,expiry_or_due_date,discount_cents,tax_basis_points,notes,terms,updated_at FROM documents WHERE kind=? ORDER BY updated_at DESC").bind(kind.as_str()).fetch_all(&self.pool).await?;
        let mut result = Vec::with_capacity(docs.len());
        for document in docs {
            let lines = self.lines(&document.id).await?;
            result.push(FullDocument { document, lines });
        }
        Ok(result)
    }

    pub async fn search_documents(
        &self,
        kind: DocumentKind,
        search: &str,
    ) -> Result<Vec<FullDocument>, sqlx::Error> {
        let documents = self.documents(kind).await?;
        let search = search.trim().to_lowercase();
        if search.is_empty() {
            return Ok(documents);
        }

        Ok(documents
            .into_iter()
            .filter(|document| {
                let header_matches = [
                    document.document.number.as_str(),
                    document.document.customer_name.as_str(),
                    document.document.customer_email.as_str(),
                    document.document.customer_phone.as_str(),
                    document.document.billing_address.as_str(),
                    document.document.issue_date.as_str(),
                    document
                        .document
                        .expiry_or_due_date
                        .as_deref()
                        .unwrap_or_default(),
                    document.document.notes.as_str(),
                    document.document.terms.as_str(),
                ]
                .iter()
                .any(|value| value.to_lowercase().contains(&search));
                let line_matches = document.lines.iter().any(|line| {
                    [
                        line.description.as_str(),
                        line.quantity.as_str(),
                        line.unit_label.as_str(),
                    ]
                    .iter()
                    .any(|value| value.to_lowercase().contains(&search))
                });
                header_matches || line_matches
            })
            .collect())
    }

    pub async fn document(
        &self,
        id: &str,
        kind: DocumentKind,
    ) -> Result<Option<FullDocument>, sqlx::Error> {
        let document: Option<Document> = sqlx::query_as("SELECT id,number,status,customer_name,customer_email,customer_phone,billing_address,issue_date,expiry_or_due_date,discount_cents,tax_basis_points,notes,terms,updated_at FROM documents WHERE id=? AND kind=?").bind(id).bind(kind.as_str()).fetch_optional(&self.pool).await?;
        match document {
            Some(document) => {
                let lines = self.lines(id).await?;
                Ok(Some(FullDocument { document, lines }))
            }
            None => Ok(None),
        }
    }
    async fn lines(&self, id: &str) -> Result<Vec<DocumentLine>, sqlx::Error> {
        sqlx::query_as("SELECT description,quantity,unit_label,unit_price_cents FROM document_lines WHERE document_id=? ORDER BY display_position").bind(id).fetch_all(&self.pool).await
    }
    pub async fn create_document(
        &self,
        kind: DocumentKind,
        valid: &ValidDocument,
    ) -> Result<String, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let settings: Settings = sqlx::query_as("SELECT business_name,address,phone,email,logo_path,default_tax_basis_points,default_turnaround_days,quote_prefix,invoice_prefix,next_quote_number,next_invoice_number FROM settings WHERE singleton=1").fetch_one(&mut *tx).await?;
        let (prefix, next) = match kind {
            DocumentKind::Quote => (&settings.quote_prefix, settings.next_quote_number),
            DocumentKind::Invoice => (&settings.invoice_prefix, settings.next_invoice_number),
        };
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let number = format!("{prefix}{next}");
        sqlx::query("INSERT INTO documents (id,kind,number,status,customer_name,customer_email,customer_phone,billing_address,issue_date,expiry_or_due_date,discount_cents,tax_basis_points,notes,terms,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(&id).bind(kind.as_str()).bind(number).bind(&valid.input.status).bind(valid.input.customer_name.trim()).bind(valid.input.customer_email.trim()).bind(valid.input.customer_phone.trim()).bind(valid.input.billing_address.trim()).bind(&valid.input.issue_date).bind(option(&valid.input.expiry_or_due_date)).bind(valid.discount_cents).bind(valid.tax_basis_points).bind(valid.input.notes.trim()).bind(valid.input.terms.trim()).bind(&now).bind(&now).execute(&mut *tx).await?;
        let counter = match kind {
            DocumentKind::Quote => "next_quote_number",
            DocumentKind::Invoice => "next_invoice_number",
        };
        let sql = format!("UPDATE settings SET {counter}={counter}+1 WHERE singleton=1");
        sqlx::query(&sql).execute(&mut *tx).await?;
        insert_lines(&mut tx, &id, valid).await?;
        tx.commit().await?;
        Ok(id)
    }
    pub async fn update_document(
        &self,
        id: &str,
        kind: DocumentKind,
        valid: &ValidDocument,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query("UPDATE documents SET status=?,customer_name=?,customer_email=?,customer_phone=?,billing_address=?,issue_date=?,expiry_or_due_date=?,discount_cents=?,tax_basis_points=?,notes=?,terms=?,updated_at=? WHERE id=? AND kind=?")
            .bind(&valid.input.status).bind(valid.input.customer_name.trim()).bind(valid.input.customer_email.trim()).bind(valid.input.customer_phone.trim()).bind(valid.input.billing_address.trim()).bind(&valid.input.issue_date).bind(option(&valid.input.expiry_or_due_date)).bind(valid.discount_cents).bind(valid.tax_basis_points).bind(valid.input.notes.trim()).bind(valid.input.terms.trim()).bind(&now).bind(id).bind(kind.as_str()).execute(&mut *tx).await?;
        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query("DELETE FROM document_lines WHERE document_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        insert_lines(&mut tx, id, valid).await?;
        tx.commit().await?;
        Ok(true)
    }
    pub async fn record_email_success(
        &self,
        id: &str,
        kind: DocumentKind,
        provider_message_id: &str,
    ) -> Result<bool, sqlx::Error> {
        Ok(sqlx::query(
            "UPDATE documents SET last_emailed_at=?,resend_message_id=? WHERE id=? AND kind=?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(provider_message_id)
        .bind(id)
        .bind(kind.as_str())
        .execute(&self.pool)
        .await?
        .rows_affected()
            == 1)
    }
}

async fn insert_lines(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    id: &str,
    valid: &ValidDocument,
) -> Result<(), sqlx::Error> {
    for (position, line) in valid.lines.iter().enumerate() {
        sqlx::query("INSERT INTO document_lines (id,document_id,display_position,description,quantity,unit_label,unit_price_cents) VALUES (?,?,?,?,?,?,?)").bind(Uuid::new_v4().to_string()).bind(id).bind(i64::try_from(position).unwrap_or(0)).bind(&line.description).bind(line.quantity.normalize().to_string()).bind(&line.unit_label).bind(line.unit_price_cents).execute(&mut **tx).await?;
    }
    Ok(())
}
fn option(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value.trim())
}

pub fn format_cents(cents: i64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

pub fn format_rate(basis_points: i64) -> String {
    format!("{}.{:02}", basis_points / 100, basis_points % 100)
}
