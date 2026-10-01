use chrono::Utc;
use sqlx::{QueryBuilder, Sqlite};
use uuid::Uuid;

use super::{JobTagProjection, Repository, Tag, events::record_job_event};

pub(super) const TAG_COLOR_KEYS: [&str; 6] = ["neutral", "blue", "green", "amber", "red", "purple"];

impl Repository {
    pub async fn job_tag_projections(
        &self,
        job_ids: &[String],
    ) -> Result<Vec<JobTagProjection>, sqlx::Error> {
        if job_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT job_tags.job_id,tags.id,tags.name,tags.color_key FROM job_tags JOIN tags ON tags.id=job_tags.tag_id WHERE job_tags.job_id IN (",
        );
        let mut separated = query.separated(",");
        for id in job_ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(") ORDER BY tags.name COLLATE NOCASE,tags.id");
        query.build_query_as().fetch_all(&self.pool).await
    }

    pub async fn tags(&self, include_archived: bool) -> Result<Vec<Tag>, sqlx::Error> {
        sqlx::query_as("SELECT id,name,color_key,archived_at,created_at FROM tags WHERE ? OR archived_at IS NULL ORDER BY name COLLATE NOCASE,id")
            .bind(include_archived)
            .fetch_all(&self.pool)
            .await
    }

    pub async fn create_tag(
        &self,
        name: &str,
        color_key: &str,
    ) -> Result<Option<Tag>, sqlx::Error> {
        let name = name.trim();
        let color_key = color_key.trim();
        if !bounded_nonempty(name, 100) || !TAG_COLOR_KEYS.contains(&color_key) {
            return Ok(None);
        }
        let id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO tags (id,name,color_key,created_at) VALUES (?,?,?,?)",
        )
        .bind(&id)
        .bind(name)
        .bind(color_key)
        .bind(&created_at)
        .execute(&self.pool)
        .await?;
        if inserted.rows_affected() == 0 {
            return Ok(None);
        }
        Ok(Some(Tag {
            id,
            name: name.to_owned(),
            color_key: color_key.to_owned(),
            archived_at: None,
            created_at,
        }))
    }

    pub async fn archive_tag(&self, id: &str) -> Result<bool, sqlx::Error> {
        Ok(
            sqlx::query("UPDATE tags SET archived_at=? WHERE id=? AND archived_at IS NULL")
                .bind(Utc::now().to_rfc3339())
                .bind(id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                == 1,
        )
    }

    pub async fn assign_tag(&self, job_id: &str, tag_id: &str) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now().to_rfc3339();
        let inserted = sqlx::query("INSERT OR IGNORE INTO job_tags (job_id,tag_id,created_at) SELECT ?,id,? FROM tags WHERE id=? AND archived_at IS NULL")
            .bind(job_id)
            .bind(&now)
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
        if inserted.rows_affected() == 1 {
            let name: String = sqlx::query_scalar("SELECT name FROM tags WHERE id=?")
                .bind(tag_id)
                .fetch_one(&mut *tx)
                .await?;
            record_job_event(
                &mut tx,
                job_id,
                "label_added",
                &format!("Label added: {name}"),
                &now,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(inserted.rows_affected() == 1)
    }

    pub async fn unassign_tag(&self, job_id: &str, tag_id: &str) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let name: Option<String> = sqlx::query_scalar("SELECT name FROM tags WHERE id=?")
            .bind(tag_id)
            .fetch_optional(&mut *tx)
            .await?;
        let deleted = sqlx::query("DELETE FROM job_tags WHERE job_id=? AND tag_id=?")
            .bind(job_id)
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
        if deleted.rows_affected() == 1
            && let Some(name) = name
        {
            let now = Utc::now().to_rfc3339();
            record_job_event(
                &mut tx,
                job_id,
                "label_removed",
                &format!("Label removed: {name}"),
                &now,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(deleted.rows_affected() == 1)
    }

    pub async fn tags_for_job(&self, job_id: &str) -> Result<Vec<Tag>, sqlx::Error> {
        sqlx::query_as("SELECT tags.id,tags.name,tags.color_key,tags.archived_at,tags.created_at FROM tags JOIN job_tags ON job_tags.tag_id=tags.id WHERE job_tags.job_id=? ORDER BY tags.name COLLATE NOCASE,tags.id")
            .bind(job_id)
            .fetch_all(&self.pool)
            .await
    }
}

fn bounded_nonempty(value: &str, max: usize) -> bool {
    let length = value.trim().chars().count();
    (1..=max).contains(&length)
}
