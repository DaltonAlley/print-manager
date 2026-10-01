use chrono::Utc;
use sqlx::{Sqlite, SqliteConnection, Transaction};

use crate::domain::{BoardLane, JobStage};

#[cfg(test)]
use super::QuoteWorkItem;
use super::{BoardAdvance, BoardReorder, Repository, events::record_job_event};

fn production_position_column(stage: JobStage) -> &'static str {
    match stage {
        JobStage::Design => "design_board_position",
        JobStage::Waiting => "waiting_board_position",
        JobStage::Print => "print_board_position",
        JobStage::Finish => "finish_board_position",
        JobStage::Complete => "complete_board_position",
    }
}

async fn ordered_board_ids(
    connection: &mut SqliteConnection,
    lane: BoardLane,
) -> Result<Vec<String>, sqlx::Error> {
    match lane {
        BoardLane::Quote => {
            sqlx::query_scalar::<_, String>("SELECT documents.id FROM documents WHERE documents.kind='quote' AND documents.status='accepted' AND NOT EXISTS (SELECT 1 FROM jobs WHERE jobs.quote_id=documents.id) ORDER BY documents.quote_board_position IS NULL,documents.quote_board_position,documents.updated_at DESC,documents.id DESC")
                .fetch_all(connection)
                .await
        }
        BoardLane::Production(JobStage::Complete) => {
            sqlx::query_scalar::<_, String>("WITH newest AS (SELECT id,complete_board_position,completed_at FROM jobs WHERE status='complete' AND completed_at IS NOT NULL ORDER BY completed_at DESC,id DESC LIMIT 10) SELECT id FROM newest ORDER BY complete_board_position IS NULL,complete_board_position,completed_at DESC,id DESC")
                .fetch_all(connection)
                .await
        }
        BoardLane::Production(stage) => {
            let column = production_position_column(stage);
            let query = format!(
                "SELECT id FROM jobs WHERE status=? AND completed_at IS NULL ORDER BY {column} IS NULL,{column},due_date IS NULL,due_date,updated_at DESC,id DESC"
            );
            sqlx::query_scalar::<_, String>(&query)
                .bind(stage.as_str())
                .fetch_all(connection)
                .await
        }
    }
}

async fn persist_board_positions(
    connection: &mut SqliteConnection,
    lane: BoardLane,
    card_ids: &[String],
) -> Result<(), sqlx::Error> {
    for (position, id) in card_ids.iter().enumerate() {
        let position = i64::try_from(position).unwrap_or(i64::MAX);
        match lane {
            BoardLane::Quote => {
                sqlx::query("UPDATE documents SET quote_board_position=? WHERE id=?")
                    .bind(position)
                    .bind(id)
                    .execute(&mut *connection)
                    .await?;
            }
            BoardLane::Production(stage) => {
                let query = format!(
                    "UPDATE jobs SET {}=? WHERE id=?",
                    production_position_column(stage)
                );
                sqlx::query(&query)
                    .bind(position)
                    .bind(id)
                    .execute(&mut *connection)
                    .await?;
            }
        }
    }
    Ok(())
}

fn reorder_existing_card(
    card_ids: &mut Vec<String>,
    card_id: &str,
    reorder: BoardReorder<'_>,
) -> bool {
    let Some(source_index) = card_ids.iter().position(|id| id == card_id) else {
        return false;
    };
    let destination_index = match reorder {
        BoardReorder::Up => source_index.saturating_sub(1),
        BoardReorder::Down => (source_index + 1).min(card_ids.len().saturating_sub(1)),
        BoardReorder::Last => card_ids.len().saturating_sub(1),
        BoardReorder::Before(target_id) | BoardReorder::After(target_id) => {
            if target_id == card_id {
                return false;
            }
            let Some(target_index) = card_ids.iter().position(|id| id == target_id) else {
                return false;
            };
            let adjusted_target = if target_index > source_index {
                target_index - 1
            } else {
                target_index
            };
            match reorder {
                BoardReorder::Before(_) => adjusted_target,
                BoardReorder::After(_) => adjusted_target + 1,
                BoardReorder::Up | BoardReorder::Down | BoardReorder::Last => unreachable!(),
            }
        }
    };
    if destination_index != source_index {
        let card = card_ids.remove(source_index);
        card_ids.insert(destination_index, card);
    }
    true
}

fn insert_board_card(card_ids: &mut Vec<String>, card_id: &str, reorder: BoardReorder<'_>) -> bool {
    let destination_index = match reorder {
        BoardReorder::Last => card_ids.len(),
        BoardReorder::Before(target_id) | BoardReorder::After(target_id) => {
            let Some(target_index) = card_ids.iter().position(|id| id == target_id) else {
                return false;
            };
            target_index + usize::from(matches!(reorder, BoardReorder::After(_)))
        }
        BoardReorder::Up | BoardReorder::Down => return false,
    };
    card_ids.insert(destination_index, card_id.to_owned());
    true
}

fn is_expected_stage_membership(
    membership: Option<(String, Option<String>, Option<String>)>,
    stage: JobStage,
) -> bool {
    membership.is_some_and(|(status, completed_at, picked_up_at)| {
        status == stage.as_str()
            && completed_at.is_some() == (stage == JobStage::Complete)
            && picked_up_at.is_none()
    })
}

impl Repository {
    #[cfg(test)]
    pub async fn unconverted_active_quotes(&self) -> Result<Vec<QuoteWorkItem>, sqlx::Error> {
        sqlx::query_as("SELECT documents.id,documents.number,documents.status,documents.customer_name FROM documents WHERE documents.kind='quote' AND documents.status='accepted' AND NOT EXISTS (SELECT 1 FROM jobs WHERE jobs.quote_id=documents.id) ORDER BY documents.quote_board_position IS NULL,documents.quote_board_position,documents.updated_at DESC,documents.id DESC")
            .fetch_all(&self.pool)
            .await
    }

    pub async fn reorder_board_card(
        &self,
        lane: BoardLane,
        card_id: &str,
        reorder: BoardReorder<'_>,
    ) -> Result<bool, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let mut card_ids = ordered_board_ids(&mut transaction, lane).await?;
        if !reorder_existing_card(&mut card_ids, card_id, reorder) {
            transaction.rollback().await?;
            return Ok(false);
        }
        persist_board_positions(&mut transaction, lane, &card_ids).await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn move_board_card(
        &self,
        source: BoardLane,
        destination: BoardLane,
        card_id: &str,
        reorder: BoardReorder<'_>,
    ) -> Result<Option<BoardAdvance>, sqlx::Error> {
        match (source, destination) {
            (BoardLane::Quote, BoardLane::Production(JobStage::Design)) => {
                let mut transaction = self.pool.begin().await?;
                let existing: Option<(String, String, Option<String>, Option<String>)> =
                    sqlx::query_as(
                        "SELECT id,status,completed_at,picked_up_at FROM jobs WHERE quote_id=?",
                    )
                    .bind(card_id)
                    .fetch_optional(&mut *transaction)
                    .await?;
                if let Some((job_id, status, completed_at, picked_up_at)) = existing {
                    if status == JobStage::Design.as_str()
                        && completed_at.is_none()
                        && picked_up_at.is_none()
                    {
                        transaction.commit().await?;
                        return Ok(Some(BoardAdvance::StartedJob(job_id)));
                    }
                    transaction.rollback().await?;
                    return Ok(None);
                }
                let mut destination_ids = ordered_board_ids(&mut transaction, destination).await?;
                if !insert_board_card(&mut destination_ids, card_id, reorder) {
                    transaction.rollback().await?;
                    return Ok(None);
                }
                let Some(job_id) = self.start_quoted_job_on(&mut transaction, card_id).await?
                else {
                    transaction.rollback().await?;
                    return Ok(None);
                };
                let inserted = destination_ids
                    .iter_mut()
                    .find(|id| id.as_str() == card_id)
                    .is_some_and(|id| {
                        id.clone_from(&job_id);
                        true
                    });
                if !inserted {
                    transaction.rollback().await?;
                    return Ok(None);
                }
                persist_board_positions(&mut transaction, destination, &destination_ids).await?;
                transaction.commit().await?;
                Ok(Some(BoardAdvance::StartedJob(job_id)))
            }
            (BoardLane::Production(_), BoardLane::Quote) | (BoardLane::Quote, _) => Ok(None),
            (BoardLane::Production(source_stage), BoardLane::Production(destination_stage)) => {
                if source_stage == destination_stage {
                    return Ok(None);
                }
                let mut transaction = self.pool.begin().await?;
                let membership: Option<(String, Option<String>, Option<String>)> =
                    sqlx::query_as("SELECT status,completed_at,picked_up_at FROM jobs WHERE id=?")
                        .bind(card_id)
                        .fetch_optional(&mut *transaction)
                        .await?;
                let expected_completion = source_stage == JobStage::Complete;
                if !is_expected_stage_membership(membership, source_stage) {
                    return rollback_move(transaction).await;
                }
                let source_ids = ordered_board_ids(&mut transaction, source).await?;
                if !source_ids.iter().any(|id| id == card_id) {
                    return rollback_move(transaction).await;
                }
                let mut destination_ids = ordered_board_ids(&mut transaction, destination).await?;
                if !insert_board_card(&mut destination_ids, card_id, reorder) {
                    transaction.rollback().await?;
                    return Ok(None);
                }
                let now = Utc::now().to_rfc3339();
                let completed_at =
                    (destination_stage == JobStage::Complete).then_some(now.as_str());
                let updated = sqlx::query("UPDATE jobs SET status=?,completed_at=?,updated_at=? WHERE id=? AND status=? AND picked_up_at IS NULL AND ((? = 1 AND completed_at IS NOT NULL) OR (? = 0 AND completed_at IS NULL))")
                    .bind(destination_stage.as_str())
                    .bind(completed_at)
                    .bind(&now)
                    .bind(card_id)
                    .bind(source_stage.as_str())
                    .bind(expected_completion)
                    .bind(expected_completion)
                    .execute(&mut *transaction)
                    .await?;
                if updated.rows_affected() != 1 {
                    return rollback_move(transaction).await;
                }
                record_move(
                    &mut transaction,
                    card_id,
                    (source_stage, destination_stage),
                    &now,
                )
                .await?;
                persist_board_positions(&mut transaction, destination, &destination_ids).await?;
                transaction.commit().await?;
                Ok(Some(BoardAdvance::Moved(destination_stage)))
            }
        }
    }
}

async fn rollback_move(
    transaction: Transaction<'_, Sqlite>,
) -> Result<Option<BoardAdvance>, sqlx::Error> {
    transaction.rollback().await?;
    Ok(None)
}

async fn record_move(
    connection: &mut SqliteConnection,
    job_id: &str,
    transition: (JobStage, JobStage),
    occurred_at: &str,
) -> Result<(), sqlx::Error> {
    let (source, destination) = transition;
    let (kind, summary) = if destination == JobStage::Complete {
        ("completed", "Job completed".to_owned())
    } else if source == JobStage::Complete {
        (
            "reopened",
            format!("Job reopened in {}", stage_label(destination)),
        )
    } else {
        (
            "stage_changed",
            format!(
                "Moved from {} to {}",
                stage_label(source),
                stage_label(destination)
            ),
        )
    };
    record_job_event(connection, job_id, kind, &summary, occurred_at).await
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
