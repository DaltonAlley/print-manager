use std::{str::FromStr, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

// Keep the checked-in migration directory bundled into every server and test binary.
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/");

#[derive(Clone, Debug)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Opens the single `SQLite` database with WAL and foreign-key enforcement.
    ///
    /// # Errors
    ///
    /// Returns a `SQLx` error if the URL is invalid or `SQLite` cannot open the database.
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        Ok(Self { pool })
    }

    /// Applies every checked-in migration that has not already run.
    ///
    /// # Errors
    ///
    /// Returns a migration error if `SQLx` cannot read or apply the schema changes.
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        MIGRATOR.run(&self.pool).await
    }

    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sqlx::Row;

    use super::*;

    #[tokio::test]
    async fn migration_creates_the_domain_tables() {
        let database = Database::connect("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        database
            .migrate()
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let rows = sqlx::query(
            "SELECT name FROM sqlite_master \
             WHERE type = ? AND name NOT LIKE ? AND name <> ? ORDER BY name",
        )
        .bind("table")
        .bind("sqlite_%")
        .bind("_sqlx_migrations")
        .fetch_all(database.pool())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        let tables = rows
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<BTreeSet<_>>();

        assert_eq!(
            tables,
            BTreeSet::from([
                "document_lines".to_owned(),
                "documents".to_owned(),
                "jobs".to_owned(),
                "job_attachments".to_owned(),
                "job_events".to_owned(),
                "job_tags".to_owned(),
                "settings".to_owned(),
                "tags".to_owned(),
            ])
        );

        let settings_columns = sqlx::query("PRAGMA table_info(settings)")
            .fetch_all(database.pool())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<BTreeSet<_>>();
        assert!(settings_columns.contains("default_turnaround_days"));
    }

    #[tokio::test]
    async fn workflow_workspace_migration_backfills_only_complete_jobs() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        sqlx::raw_sql(include_str!("../migrations/0001_initial.sql"))
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        sqlx::raw_sql(include_str!("../migrations/0002_production_lifecycle.sql"))
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        for (id, status, updated_at) in [
            (
                "00000000-0000-4000-8000-000000000001",
                "complete",
                "2026-01-02T03:04:05Z",
            ),
            ("00000000-0000-4000-8000-000000000002", "complete", ""),
            (
                "00000000-0000-4000-8000-000000000003",
                "design",
                "2026-01-03T03:04:05Z",
            ),
        ] {
            sqlx::query("INSERT INTO jobs (id,title,customer_name,category,status,notes,created_at,updated_at) VALUES (?,'Job','Customer','Print',?,'','',?)")
                .bind(id)
                .bind(status)
                .bind(updated_at)
                .execute(&pool)
                .await
                .unwrap_or_else(|error| panic!("{error}"));
        }
        sqlx::raw_sql(include_str!("../migrations/0003_workflow_workspace.sql"))
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let values: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT id,completed_at FROM jobs ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(values[0].1.as_deref(), Some("2026-01-02T03:04:05Z"));
        assert_eq!(values[1].1.as_deref(), Some("1970-01-01T00:00:00Z"));
        assert_eq!(values[2].1, None);

        let index_sql: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type='index' AND name='jobs_completed_idx'",
        )
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(index_sql.contains("WHERE completed_at IS NOT NULL"));
    }

    #[tokio::test]
    async fn board_sorting_migration_adds_lane_specific_positions() {
        let database = Database::connect("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        database
            .migrate()
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let document_columns: BTreeSet<String> = sqlx::query("PRAGMA table_info(documents)")
            .fetch_all(database.pool())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .iter()
            .map(|row| row.get("name"))
            .collect();
        let job_columns: BTreeSet<String> = sqlx::query("PRAGMA table_info(jobs)")
            .fetch_all(database.pool())
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .iter()
            .map(|row| row.get("name"))
            .collect();

        assert!(document_columns.contains("quote_board_position"));
        assert!(job_columns.contains("active_board_position"));
        assert!(job_columns.contains("complete_board_position"));
        assert!(job_columns.contains("design_board_position"));
        assert!(job_columns.contains("waiting_board_position"));
        assert!(job_columns.contains("print_board_position"));
        assert!(job_columns.contains("finish_board_position"));
    }

    #[tokio::test]
    async fn event_history_migration_backfills_only_durable_lifecycle_facts() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        for migration in [
            include_str!("../migrations/0001_initial.sql"),
            include_str!("../migrations/0002_production_lifecycle.sql"),
            include_str!("../migrations/0003_workflow_workspace.sql"),
            include_str!("../migrations/0004_board_sorting.sql"),
            include_str!("../migrations/0005_production_stage_board_sorting.sql"),
        ] {
            sqlx::raw_sql(migration)
                .execute(&pool)
                .await
                .unwrap_or_else(|error| panic!("{error}"));
        }
        let job_id = "00000000-0000-4000-8000-000000000001";
        sqlx::query("INSERT INTO jobs (id,title,customer_name,category,status,picked_up_at,completed_at,notes,created_at,updated_at) VALUES (?,'Backfill','Customer','Print','complete','2026-01-03T00:00:00Z','2026-01-02T00:00:00Z','','2026-01-01T00:00:00Z','2026-01-04T00:00:00Z')")
            .bind(job_id)
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        sqlx::raw_sql(include_str!("../migrations/0006_job_event_history.sql"))
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let facts: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT kind,summary,occurred_at FROM job_events WHERE job_id=? ORDER BY occurred_at,id",
        )
        .bind(job_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            facts,
            vec![
                (
                    "created".into(),
                    "Job created".into(),
                    "2026-01-01T00:00:00Z".into()
                ),
                (
                    "completed".into(),
                    "Job completed".into(),
                    "2026-01-02T00:00:00Z".into()
                ),
                (
                    "pickup_confirmed".into(),
                    "Pickup confirmed".into(),
                    "2026-01-03T00:00:00Z".into()
                ),
            ]
        );
    }

    #[tokio::test]
    async fn connections_enable_wal_and_foreign_keys() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let path = directory.path().join("print-manager.db");
        let database = Database::connect(&format!("sqlite:{}", path.display()))
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(database.pool())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(database.pool())
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(journal_mode, "wal");
        assert_eq!(foreign_keys, 1);
        assert!(path.is_file());
    }

    #[tokio::test]
    #[allow(clippy::type_complexity)]
    async fn lifecycle_migration_maps_legacy_stages_without_inventing_completion_facts() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        sqlx::raw_sql(include_str!("../migrations/0001_initial.sql"))
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        sqlx::query("INSERT INTO documents (id,kind,number,status,customer_name,issue_date,created_at,updated_at) VALUES (?,'quote','Q-1','draft','Customer','2026-08-20','','')")
            .bind("10000000-0000-4000-8000-000000000000")
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        for (index, status) in ["queued", "in_progress", "ready", "done"]
            .into_iter()
            .enumerate()
        {
            let quote_id = (index < 2).then_some("10000000-0000-4000-8000-000000000000");
            sqlx::query("INSERT INTO jobs (id,title,customer_name,category,status,quote_id,notes,created_at,updated_at) VALUES (?,?,?,?,?,?,'','',?)")
                .bind(format!("00000000-0000-4000-8000-{index:012}"))
                .bind(status)
                .bind("Customer")
                .bind("Category")
                .bind(status)
                .bind(quote_id)
                .bind(format!("{index}"))
                .execute(&pool)
                .await
                .unwrap_or_else(|error| panic!("{error}"));
        }

        sqlx::raw_sql(include_str!("../migrations/0002_production_lifecycle.sql"))
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let rows: Vec<(String, Option<String>, Option<String>, Option<String>)> =
            sqlx::query_as("SELECT status,picked_up_at,invoice_id,quote_id FROM jobs ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            rows,
            vec![
                (
                    "design".into(),
                    None,
                    None,
                    Some("10000000-0000-4000-8000-000000000000".into())
                ),
                ("print".into(), None, None, None),
                ("complete".into(), None, None, None),
                ("complete".into(), None, None, None),
            ]
        );
    }
}
