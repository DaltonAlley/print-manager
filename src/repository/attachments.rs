use chrono::Utc;
use sqlx::{QueryBuilder, Sqlite};
use uuid::Uuid;

use crate::domain::AttachmentIntegrity;

use super::{
    JobAttachment, JobAttachmentCount, JobAttachmentProjection, NewJobAttachment, Repository,
    events::record_job_event,
};

impl NewJobAttachment<'_> {
    pub(super) fn is_valid(&self) -> bool {
        bounded_nonempty(self.original_name, 255)
            && bounded_nonempty(self.stored_name, 255)
            && bounded_nonempty(self.media_type, 255)
            && self.byte_size >= 0
            && self.sha256.len() == 64
            && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    }
}

impl Repository {
    pub async fn job_attachment_counts(
        &self,
        job_ids: &[String],
    ) -> Result<Vec<JobAttachmentCount>, sqlx::Error> {
        if job_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT job_id,COUNT(*) AS attachment_count FROM job_attachments WHERE job_id IN (",
        );
        let mut separated = query.separated(",");
        for id in job_ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(") GROUP BY job_id");
        query.build_query_as().fetch_all(&self.pool).await
    }

    pub async fn job_attachment_projections(
        &self,
        job_ids: &[String],
    ) -> Result<Vec<JobAttachmentProjection>, sqlx::Error> {
        if job_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT id,job_id,original_name,media_type,byte_size FROM job_attachments WHERE job_id IN (",
        );
        let mut separated = query.separated(",");
        for id in job_ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(") ORDER BY job_id,created_at,id");
        query.build_query_as().fetch_all(&self.pool).await
    }
    pub async fn create_attachment(
        &self,
        job_id: &str,
        metadata: &NewJobAttachment<'_>,
    ) -> Result<Option<JobAttachment>, sqlx::Error> {
        if !metadata.is_valid() {
            return Ok(None);
        }
        let attachment = JobAttachment {
            id: Uuid::new_v4().to_string(),
            job_id: job_id.to_owned(),
            original_name: metadata.original_name.trim().to_owned(),
            stored_name: metadata.stored_name.trim().to_owned(),
            media_type: metadata.media_type.trim().to_owned(),
            byte_size: metadata.byte_size,
            sha256: metadata.sha256.to_ascii_lowercase(),
            created_at: Utc::now().to_rfc3339(),
        };
        let mut tx = self.pool.begin().await?;
        let inserted = sqlx::query("INSERT INTO job_attachments (id,job_id,original_name,stored_name,media_type,byte_size,sha256,created_at) SELECT ?,jobs.id,?,?,?,?,?,? FROM jobs WHERE jobs.id=? AND jobs.completed_at IS NULL")
            .bind(&attachment.id)
            .bind(&attachment.original_name)
            .bind(&attachment.stored_name)
            .bind(&attachment.media_type)
            .bind(attachment.byte_size)
            .bind(&attachment.sha256)
            .bind(&attachment.created_at)
            .bind(&attachment.job_id)
            .execute(&mut *tx)
            .await?;
        if inserted.rows_affected() == 0 {
            let job_exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM jobs WHERE id=?)")
                    .bind(&attachment.job_id)
                    .fetch_one(&mut *tx)
                    .await?;
            tx.rollback().await?;
            return if job_exists {
                Ok(None)
            } else {
                Err(sqlx::Error::RowNotFound)
            };
        }
        record_job_event(
            &mut tx,
            job_id,
            "attachment_added",
            &format!("Attachment added: {}", attachment.original_name),
            &attachment.created_at,
        )
        .await?;
        tx.commit().await?;
        Ok(Some(attachment))
    }

    pub async fn attachments_for_job(
        &self,
        job_id: &str,
    ) -> Result<Vec<JobAttachment>, sqlx::Error> {
        sqlx::query_as("SELECT id,job_id,original_name,stored_name,media_type,byte_size,sha256,created_at FROM job_attachments WHERE job_id=? ORDER BY created_at,id")
            .bind(job_id)
            .fetch_all(&self.pool)
            .await
    }

    pub async fn attachment(&self, id: &str) -> Result<Option<JobAttachment>, sqlx::Error> {
        sqlx::query_as("SELECT id,job_id,original_name,stored_name,media_type,byte_size,sha256,created_at FROM job_attachments WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    pub(crate) async fn all_attachment_integrity(
        &self,
    ) -> Result<Vec<AttachmentIntegrity>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (String, i64, String)>(
            "SELECT stored_name,byte_size,sha256 FROM job_attachments ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(stored_name, byte_size, sha256)| AttachmentIntegrity {
                stored_name,
                byte_size,
                sha256,
            })
            .collect())
    }

    pub async fn delete_attachment(&self, id: &str) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let attachment: Option<(String, String)> = sqlx::query_as(
            "SELECT job_id,original_name FROM job_attachments WHERE id=? AND EXISTS (SELECT 1 FROM jobs WHERE jobs.id=job_attachments.job_id AND jobs.completed_at IS NULL)",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let deleted = sqlx::query("DELETE FROM job_attachments WHERE id=? AND EXISTS (SELECT 1 FROM jobs WHERE jobs.id=job_attachments.job_id AND jobs.completed_at IS NULL)")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if deleted.rows_affected() == 1
            && let Some((job_id, name)) = attachment
        {
            let now = Utc::now().to_rfc3339();
            record_job_event(
                &mut tx,
                &job_id,
                "attachment_deleted",
                &format!("Attachment deleted: {name}"),
                &now,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(deleted.rows_affected() == 1)
    }
}

fn bounded_nonempty(value: &str, max: usize) -> bool {
    let length = value.trim().chars().count();
    (1..=max).contains(&length)
}
