use sqlx::SqliteConnection;

use super::{JobEvent, Repository};

pub(super) async fn record_job_event(
    connection: &mut SqliteConnection,
    job_id: &str,
    kind: &str,
    summary: &str,
    occurred_at: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO job_events (job_id,kind,summary,occurred_at) VALUES (?,?,?,?)")
        .bind(job_id)
        .bind(kind)
        .bind(summary)
        .bind(occurred_at)
        .execute(connection)
        .await?;
    Ok(())
}

impl Repository {
    pub async fn job_events(&self, job_id: &str) -> Result<Vec<JobEvent>, sqlx::Error> {
        sqlx::query_as(
            "SELECT kind,summary,occurred_at FROM job_events WHERE job_id=? ORDER BY occurred_at DESC,id DESC",
        )
        .bind(job_id)
        .fetch_all(&self.pool)
        .await
    }
}
