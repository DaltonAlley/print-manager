use super::*;
use crate::database::Database;
use crate::domain::{BoardLane, DocumentKind, JobCardUpdateInput, JobInput};

use super::tags::TAG_COLOR_KEYS;
async fn repo() -> Repository {
    let db = Database::connect("sqlite::memory:")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    db.migrate().await.unwrap_or_else(|e| panic!("{e}"));
    let repo = Repository::new(db.pool().clone());
    repo.ensure_settings()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    repo
}

#[test]
fn business_date_uses_the_shop_timezone_at_utc_midnight_and_dst_boundaries() {
    let utc_midnight = "2026-09-04T00:30:00Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        business_date_at(utc_midnight, chrono_tz::America::New_York).to_string(),
        "2026-09-03"
    );

    for instant in ["2026-03-08T06:59:00Z", "2026-03-08T07:01:00Z"] {
        let instant = instant
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            business_date_at(instant, chrono_tz::America::New_York).to_string(),
            "2026-03-08"
        );
    }

    for instant in ["2026-11-01T05:30:00Z", "2026-11-01T06:30:00Z"] {
        let instant = instant
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            business_date_at(instant, chrono_tz::America::New_York).to_string(),
            "2026-11-01"
        );
    }
}

#[tokio::test]
async fn card_contact_notes_and_comments_are_saved_and_audited() {
    let repo = repo().await;
    let id = repo
        .save_job(
            None,
            &JobInput {
                title: "Window graphics".into(),
                customer_name: "Acme".into(),
                category: "Signs".into(),
                ..JobInput::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        repo.update_job_card(
            &id,
            &JobCardUpdateInput {
                customer_name: "Acme West".into(),
                customer_phone: "+1 555 0100".into(),
                customer_email: "buyer@example.test".into(),
                notes: "Use the approved blue".into(),
            },
        )
        .await
        .unwrap_or(false)
    );
    assert!(
        repo.add_job_comment(&id, "Customer approved the proof")
            .await
            .unwrap_or(false)
    );

    let job = repo
        .job(&id)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("saved job disappeared"));
    assert_eq!(job.customer_name, "Acme West");
    assert_eq!(job.customer_phone, "+1 555 0100");
    assert_eq!(job.customer_email, "buyer@example.test");
    assert_eq!(job.notes, "Use the approved blue");
    let events = repo.job_events(&id).await.unwrap_or_default();
    assert!(events.iter().any(|event| event.kind == "details_edited"));
    assert!(events.iter().any(|event| {
        event.kind == "comment" && event.summary == "Customer approved the proof"
    }));
}
fn input(name: &str) -> DocumentInput {
    DocumentInput {
        customer_name: name.into(),
        issue_date: "2026-08-20".into(),
        lines: vec![LineInput {
            description: "Cards".into(),
            quantity: "2.5".into(),
            unit_label: "box".into(),
            unit_price: "10.00".into(),
        }],
        ..DocumentInput::default()
    }
}

#[tokio::test]
async fn document_search_finds_saved_customer_and_print_details_case_insensitively() {
    let repo = repo().await;
    let mut quote = input("Remembered Customer");
    quote.customer_phone = "+1 555 0142".into();
    quote.lines[0].description = "Outdoor banner".into();
    quote.notes = "Call back about the blue proof".into();
    let quote_id = repo
        .create_document(
            DocumentKind::Quote,
            &quote
                .validate(DocumentKind::Quote)
                .unwrap_or_else(|errors| panic!("{errors:?}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    repo.create_document(
        DocumentKind::Quote,
        &input("Another Customer")
            .validate(DocumentKind::Quote)
            .unwrap_or_else(|errors| panic!("{errors:?}")),
    )
    .await
    .unwrap_or_else(|error| panic!("{error}"));

    let matches = repo
        .search_documents(DocumentKind::Quote, "  OUTDOOR  ")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].document.id, quote_id);

    let matches = repo
        .search_documents(DocumentKind::Quote, "0142")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].document.id, quote_id);

    let matches = repo
        .search_documents(DocumentKind::Quote, "blue proof")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].document.id, quote_id);

    let all = repo
        .search_documents(DocumentKind::Quote, "")
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn quote_acceptance_records_the_decision_without_reopening_declined_work() {
    let repo = repo().await;
    let quote = repo
        .create_document(
            DocumentKind::Quote,
            &input("Accepted customer")
                .validate(DocumentKind::Quote)
                .unwrap_or_else(|errors| panic!("{errors:?}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(repo.accept_quote(&quote).await.unwrap_or(false));
    assert_eq!(
        repo.document(&quote, DocumentKind::Quote)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| panic!())
            .document
            .status,
        "accepted"
    );

    let declined = repo
        .create_document(
            DocumentKind::Quote,
            &input("Declined customer")
                .validate(DocumentKind::Quote)
                .unwrap_or_else(|errors| panic!("{errors:?}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(repo.decline_quote(&declined).await.unwrap_or(false));
    assert!(!repo.accept_quote(&declined).await.unwrap_or(true));
    assert_eq!(
        repo.document(&declined, DocumentKind::Quote)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| panic!())
            .document
            .status,
        "declined"
    );
}

#[tokio::test]
async fn invoice_before_job_keeps_edits_and_converges_to_one_workflow() {
    let repo = repo().await;
    let valid = input("Acme")
        .validate(DocumentKind::Quote)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let quote = repo
        .create_document(DocumentKind::Quote, &valid)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let invoice = repo
        .copy_quote_to_invoice(&quote)
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .unwrap_or_default();
    assert_eq!(
        repo.copy_quote_to_invoice(&quote)
            .await
            .unwrap_or_else(|e| panic!("{e}")),
        Some(invoice.clone())
    );
    let mut changed = input("Different");
    changed.lines[0].unit_price = "99.00".into();
    repo.update_document(
        &invoice,
        DocumentKind::Invoice,
        &changed
            .validate(DocumentKind::Invoice)
            .unwrap_or_else(|e| panic!("{e:?}")),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    let job = repo
        .start_job_from_quote(&quote)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    assert_eq!(
        repo.start_job_from_quote(&quote)
            .await
            .unwrap_or_else(|error| panic!("{error}")),
        Some(job.clone())
    );
    assert_eq!(
        repo.copy_quote_to_invoice(&quote)
            .await
            .unwrap_or_else(|error| panic!("{error}")),
        Some(invoice.clone())
    );
    let linked_job = repo
        .job(&job)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert_eq!(linked_job.quote_id.as_deref(), Some(quote.as_str()));
    assert_eq!(linked_job.invoice_id.as_deref(), Some(invoice.as_str()));
    let edited_invoice = repo
        .document(&invoice, DocumentKind::Invoice)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert_eq!(edited_invoice.document.customer_name, "Different");
    assert_eq!(edited_invoice.lines[0].unit_price_cents, 9_900);
    assert_eq!(
        repo.document(&quote, DocumentKind::Quote)
            .await
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!())
            .document
            .customer_name,
        "Acme"
    );
    assert!(repo.set_job_status(&job, "complete").await.unwrap_or(false));
    assert!(repo.set_job_picked_up(&job, true).await.unwrap_or(false));
    assert!(repo.mark_paid(&invoice).await.unwrap_or(false));
    let closed = repo
        .job(&job)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert_eq!(closed.status, "complete");
    assert!(closed.picked_up_at.is_some());
    assert_eq!(closed.invoice_status.as_deref(), Some("paid"));
}

#[tokio::test]
async fn picked_up_jobs_reject_stage_changes_before_and_after_payment() {
    let repo = repo().await;
    for paid in [false, true] {
        let job = repo
            .save_job(
                None,
                &JobInput {
                    title: format!("Picked up {paid}"),
                    customer_name: "Acme".into(),
                    category: "Posters".into(),
                    ..JobInput::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let invoice = repo
            .create_or_open_job_invoice(&job)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!());
        assert!(repo.set_job_status(&job, "complete").await.unwrap_or(false));
        assert!(repo.set_job_picked_up(&job, true).await.unwrap_or(false));
        if paid {
            assert!(repo.mark_paid(&invoice).await.unwrap_or(false));
        }

        assert!(!repo.set_job_status(&job, "finish").await.unwrap_or(true));
        let stored = repo
            .job(&job)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| panic!());
        assert_eq!(stored.status, "complete");
        assert!(stored.picked_up_at.is_some());
        assert_eq!(
            stored.invoice_status.as_deref(),
            Some(if paid { "paid" } else { "draft" })
        );
    }
}

#[tokio::test]
async fn concurrent_quote_actions_create_one_linked_job_and_invoice() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let database = Database::connect(&format!(
        "sqlite:{}",
        directory.path().join("workflow.db").display()
    ))
    .await
    .unwrap_or_else(|error| panic!("{error}"));
    database
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let repo = Repository::new(database.pool().clone());
    repo.ensure_settings()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let quote = repo
        .create_document(
            DocumentKind::Quote,
            &input("Concurrent")
                .validate(DocumentKind::Quote)
                .unwrap_or_else(|errors| panic!("{errors:?}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    let (invoice, job) = tokio::join!(
        repo.copy_quote_to_invoice(&quote),
        repo.start_job_from_quote(&quote)
    );
    let invoice = invoice
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    let job = job
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    let linked = repo
        .job(&job)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert_eq!(linked.invoice_id.as_deref(), Some(invoice.as_str()));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM documents WHERE source_quote_id=?",)
            .bind(&quote)
            .fetch_one(&repo.pool)
            .await
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM jobs WHERE quote_id=?")
            .bind(&quote)
            .fetch_one(&repo.pool)
            .await
            .unwrap_or_default(),
        1
    );
}
#[tokio::test]
async fn every_production_stage_and_derived_lifecycle_works() {
    let repo = repo().await;
    let input = JobInput {
        title: "Poster".into(),
        customer_name: "Acme".into(),
        category: "Wide format".into(),
        status: "design".into(),
        ..JobInput::default()
    };
    let id = repo
        .save_job(None, &input)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    for status in ["waiting", "print", "finish", "complete", "design"] {
        assert!(repo.set_job_status(&id, status).await.unwrap_or(false));
        assert_eq!(
            repo.job(&id)
                .await
                .unwrap_or(None)
                .unwrap_or_else(|| panic!())
                .status,
            status
        );
    }
    repo.set_job_status(&id, "complete").await.unwrap_or(false);
    assert!(
        repo.board_jobs(&JobFilter::default())
            .await
            .unwrap_or_default()
            .iter()
            .any(|job| job.id == id)
    );
    assert!(repo.set_job_picked_up(&id, true).await.unwrap_or(false));
    let picked_up = repo
        .job(&id)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert!(picked_up.picked_up_at.is_some());
    assert_ne!(picked_up.invoice_status.as_deref(), Some("paid"));
    assert_eq!(
        repo.job_counts().await.unwrap_or_default(),
        JobCounts {
            total: 1,
            active: 0
        }
    );
}

#[tokio::test]
async fn board_descriptor_filters_use_authoritative_job_and_invoice_fields() {
    let repo = repo().await;
    let paid = repo
        .save_job(
            None,
            &JobInput {
                title: "Vinyl banner".into(),
                customer_name: "Paid customer".into(),
                category: "Banner".into(),
                material: "13 oz vinyl".into(),
                ..JobInput::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let unpaid = repo
        .save_job(
            None,
            &JobInput {
                title: "Foam board sign".into(),
                customer_name: "Unpaid customer".into(),
                category: "Sign".into(),
                material: "Foam board".into(),
                ..JobInput::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let no_invoice = repo
        .save_job(None, &job_input("No invoice"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let paid_invoice = repo
        .create_or_open_job_invoice(&paid)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    repo.create_or_open_job_invoice(&unpaid)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    assert!(repo.mark_paid(&paid_invoice).await.unwrap_or(false));

    let filtered_ids = |jobs: Vec<Job>| jobs.into_iter().map(|job| job.id).collect::<Vec<_>>();
    let material_ids = filtered_ids(
        repo.board_jobs(&JobFilter {
            material: "13 oz vinyl".into(),
            ..JobFilter::default()
        })
        .await
        .unwrap_or_default(),
    );
    assert_eq!(material_ids.as_slice(), std::slice::from_ref(&paid));
    for (billing, expected) in [
        ("paid", paid.as_str()),
        ("unpaid", unpaid.as_str()),
        ("none", no_invoice.as_str()),
    ] {
        assert_eq!(
            filtered_ids(
                repo.board_jobs(&JobFilter {
                    billing: billing.into(),
                    ..JobFilter::default()
                })
                .await
                .unwrap_or_default()
            ),
            [expected]
        );
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn quote_job_invoice_links_are_authoritative_and_idempotent() {
    let repo = repo().await;
    let mut quote_input = input("Acme");
    quote_input.status = "accepted".into();
    let quote = repo
        .create_document(
            DocumentKind::Quote,
            &quote_input
                .validate(DocumentKind::Quote)
                .unwrap_or_else(|errors| panic!("{errors:?}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        repo.unconverted_active_quotes()
            .await
            .unwrap_or_default()
            .len(),
        1
    );
    let job = repo
        .start_job_from_quote(&quote)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    assert_eq!(
        repo.start_job_from_quote(&quote)
            .await
            .unwrap_or_else(|error| panic!("{error}")),
        Some(job.clone())
    );
    assert!(
        repo.unconverted_active_quotes()
            .await
            .unwrap_or_default()
            .is_empty()
    );

    let invoice = repo
        .copy_quote_to_invoice(&quote)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    let linked = repo
        .job(&job)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert_eq!(linked.invoice_id.as_deref(), Some(invoice.as_str()));
    assert_eq!(linked.status, "design");
    assert!(linked.picked_up_at.is_none());
    let direct_job = repo
        .save_job(
            None,
            &JobInput {
                title: "Direct job".into(),
                customer_name: "Walk-in".into(),
                category: "Digital".into(),
                ..JobInput::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let direct_invoice = repo
        .create_or_open_job_invoice(&direct_job)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!());
    assert_eq!(
        repo.create_or_open_job_invoice(&direct_job)
            .await
            .unwrap_or_else(|error| panic!("{error}")),
        Some(direct_invoice.clone())
    );
    let direct_document = repo
        .document(&direct_invoice, DocumentKind::Invoice)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert_eq!(direct_document.document.customer_name, "Walk-in");
    assert_eq!(direct_document.lines[0].description, "Direct job");
    assert_eq!(
        repo.job(&direct_job)
            .await
            .unwrap_or(None)
            .and_then(|job| job.invoice_id),
        Some(direct_invoice)
    );

    assert!(!repo.set_job_picked_up(&job, true).await.unwrap_or(false));
    assert!(repo.set_job_status(&job, "complete").await.unwrap_or(false));
    assert!(repo.set_job_picked_up(&job, true).await.unwrap_or(false));
    let picked_up = repo
        .job(&job)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert!(picked_up.picked_up_at.is_some());
    assert_ne!(picked_up.invoice_status.as_deref(), Some("paid"));
    assert!(repo.mark_paid(&invoice).await.unwrap_or(false));
    let closed = repo
        .job(&job)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert!(closed.picked_up_at.is_some());
    assert_eq!(closed.invoice_status.as_deref(), Some("paid"));
}

fn job_input(title: &str) -> JobInput {
    JobInput {
        title: title.into(),
        customer_name: "Customer".into(),
        category: "Print".into(),
        ..JobInput::default()
    }
}

#[tokio::test]
async fn manual_sorting_persists_independently_in_quote_and_active_lanes() {
    let repo = repo().await;
    for name in ["Quote A", "Quote B", "Quote C"] {
        let mut quote_input = input(name);
        quote_input.status = "accepted".into();
        let quote = quote_input
            .validate(DocumentKind::Quote)
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        repo.create_document(DocumentKind::Quote, &quote)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }
    let quotes = repo
        .unconverted_active_quotes()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|quote| quote.id)
        .collect::<Vec<_>>();
    assert!(
        repo.reorder_board_card(
            BoardLane::Quote,
            &quotes[2],
            BoardReorder::Before(&quotes[0])
        )
        .await
        .unwrap_or(false)
    );
    assert_eq!(
        repo.unconverted_active_quotes()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|quote| quote.id)
            .collect::<Vec<_>>(),
        vec![quotes[2].clone(), quotes[0].clone(), quotes[1].clone()]
    );

    for name in ["Job A", "Job B", "Job C"] {
        repo.save_job(None, &job_input(name))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }
    let active = repo
        .board_jobs(&JobFilter::default())
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|job| job.id)
        .collect::<Vec<_>>();
    assert!(
        repo.reorder_board_card(
            BoardLane::Production(JobStage::Design),
            &active[2],
            BoardReorder::Up
        )
        .await
        .unwrap_or(false)
    );
    assert_eq!(
        repo.board_jobs(&JobFilter::default())
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|job| job.id)
            .collect::<Vec<_>>(),
        vec![active[0].clone(), active[2].clone(), active[1].clone()]
    );
    assert!(
        repo.reorder_board_card(
            BoardLane::Production(JobStage::Design),
            &active[0],
            BoardReorder::Last
        )
        .await
        .unwrap_or(false)
    );
    assert_eq!(
        repo.board_jobs(&JobFilter::default())
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|job| job.id)
            .collect::<Vec<_>>(),
        vec![active[2].clone(), active[1].clone(), active[0].clone()]
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn board_moves_are_idempotent_atomic_bidirectional_and_deterministically_ordered() {
    let repo = repo().await;
    let quote = input("Forward quote")
        .validate(DocumentKind::Quote)
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    let quote = repo
        .create_document(DocumentKind::Quote, &quote)
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        repo.move_board_card(
            BoardLane::Quote,
            BoardLane::Production(JobStage::Complete),
            &quote,
            BoardReorder::Last,
        )
        .await
        .unwrap_or(None),
        None
    );
    let first = repo
        .move_board_card(
            BoardLane::Quote,
            BoardLane::Production(JobStage::Design),
            &quote,
            BoardReorder::Last,
        )
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    let second = repo
        .move_board_card(
            BoardLane::Quote,
            BoardLane::Production(JobStage::Design),
            &quote,
            BoardReorder::Last,
        )
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    let (BoardAdvance::StartedJob(first), BoardAdvance::StartedJob(second)) = (first, second)
    else {
        panic!();
    };
    assert_eq!(first, second);
    assert_eq!(
        repo.move_board_card(
            BoardLane::Production(JobStage::Design),
            BoardLane::Production(JobStage::Complete),
            &first,
            BoardReorder::Last,
        )
        .await
        .unwrap_or(None),
        Some(BoardAdvance::Moved(JobStage::Complete))
    );
    assert_eq!(
        repo.move_board_card(
            BoardLane::Production(JobStage::Complete),
            BoardLane::Production(JobStage::Design),
            &first,
            BoardReorder::Last,
        )
        .await
        .unwrap_or(None),
        Some(BoardAdvance::Moved(JobStage::Design))
    );
    let reopened = repo
        .job(&first)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert_eq!(reopened.status, "design");
    assert!(reopened.completed_at.is_none());

    assert_eq!(
        repo.move_board_card(
            BoardLane::Production(JobStage::Design),
            BoardLane::Production(JobStage::Waiting),
            &first,
            BoardReorder::Before("missing"),
        )
        .await
        .unwrap_or(None),
        None
    );
    assert_eq!(
        repo.job(&first).await.unwrap_or(None).map(|job| job.status),
        Some("design".into())
    );

    for title in ["Waiting A", "Waiting B"] {
        let id = repo
            .save_job(None, &job_input(title))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(repo.set_job_status(&id, "waiting").await.unwrap_or(false));
    }
    let waiting = repo
        .board_jobs(&JobFilter::default())
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|job| job.status == "waiting")
        .map(|job| job.id)
        .collect::<Vec<_>>();
    assert_eq!(
        repo.move_board_card(
            BoardLane::Production(JobStage::Design),
            BoardLane::Production(JobStage::Waiting),
            &first,
            BoardReorder::Before(&waiting[1]),
        )
        .await
        .unwrap_or(None),
        Some(BoardAdvance::Moved(JobStage::Waiting))
    );
    let waiting_order = repo
        .board_jobs(&JobFilter::default())
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|job| job.status == "waiting")
        .map(|job| job.id)
        .collect::<Vec<_>>();
    assert_eq!(
        waiting_order,
        vec![waiting[0].clone(), first.clone(), waiting[1].clone()]
    );
}

#[tokio::test]
async fn complete_sorting_keeps_newest_ten_membership_and_never_changes_lifecycle() {
    let repo = repo().await;
    let active = repo
        .save_job(None, &job_input("Active"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        !repo
            .reorder_board_card(
                BoardLane::Production(JobStage::Complete),
                &active,
                BoardReorder::Up
            )
            .await
            .unwrap_or(true)
    );
    let active_job = repo
        .job(&active)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert_eq!(active_job.status, "design");
    assert!(active_job.completed_at.is_none());

    for index in 0..11 {
        let id = repo
            .save_job(None, &job_input(&format!("Complete {index}")))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(repo.complete_job(&id).await.unwrap_or(false));
        sqlx::query("UPDATE jobs SET completed_at=? WHERE id=?")
            .bind(format!("2026-08-20T00:00:{index:02}Z"))
            .bind(id)
            .execute(&repo.pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }
    let newest = repo
        .board_jobs(&JobFilter::default())
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|job| job.completed_at.is_some())
        .collect::<Vec<_>>();
    let newest_ids = newest.iter().map(|job| job.id.clone()).collect::<Vec<_>>();
    assert!(
        repo.reorder_board_card(
            BoardLane::Production(JobStage::Complete),
            &newest_ids[9],
            BoardReorder::Before(&newest_ids[0]),
        )
        .await
        .unwrap_or(false)
    );
    let reordered = repo
        .board_jobs(&JobFilter::default())
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|job| job.completed_at.is_some())
        .collect::<Vec<_>>();
    assert_eq!(reordered[0].id, newest_ids[9]);
    assert_eq!(
        reordered
            .iter()
            .map(|job| job.id.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        newest_ids
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    assert_eq!(
        repo.filtered_archived_jobs(&ArchiveFilter::default(), 0, 25)
            .await
            .unwrap_or_default()
            .len(),
        1
    );
    assert!(reordered.iter().all(|job| job.completed_at.is_some()));
}

#[tokio::test]
async fn completion_drives_active_recent_and_archive_membership() {
    let repo = repo().await;
    let active = repo
        .save_job(None, &job_input("Active"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let mut completed = Vec::new();
    for index in 0..11 {
        let id = repo
            .save_job(None, &job_input(&format!("Complete {index}")))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(repo.complete_job(&id).await.unwrap_or(false));
        sqlx::query("UPDATE jobs SET completed_at=? WHERE id=?")
            .bind(format!("2026-08-20T00:00:{index:02}Z"))
            .bind(&id)
            .execute(&repo.pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        completed.push(id);
    }
    let board = repo
        .board_jobs(&JobFilter::default())
        .await
        .unwrap_or_default();
    assert_eq!(
        board
            .iter()
            .filter(|job| job.completed_at.is_none())
            .map(|job| job.id.clone())
            .collect::<Vec<_>>(),
        vec![active]
    );
    let recent = board
        .into_iter()
        .filter(|job| job.completed_at.is_some())
        .collect::<Vec<_>>();
    assert_eq!(recent.len(), 10);
    assert_eq!(recent[0].id, completed[10]);
    assert_eq!(recent[9].id, completed[1]);
    let archive = repo
        .filtered_archived_jobs(&ArchiveFilter::default(), 0, 25)
        .await
        .unwrap_or_default();
    assert_eq!(archive.len(), 1);
    assert_eq!(archive[0].id, completed[0]);

    let before = recent[0].completed_at.clone();
    let mut edited = job_input("Edited without moving");
    edited.status = "design".into();
    repo.save_job(Some(&recent[0].id), &edited)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        repo.job(&recent[0].id)
            .await
            .unwrap_or(None)
            .and_then(|job| job.completed_at),
        before
    );
    let invoice = repo
        .create_or_open_job_invoice(&recent[0].id)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    let invoice_edit = input("Invoice edit")
        .validate(DocumentKind::Invoice)
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    assert!(
        repo.update_document(&invoice, DocumentKind::Invoice, &invoice_edit)
            .await
            .unwrap_or(false)
    );
    assert!(
        repo.set_job_picked_up(&recent[0].id, true)
            .await
            .unwrap_or(false)
    );
    assert!(
        repo.set_job_picked_up(&recent[0].id, false)
            .await
            .unwrap_or(false)
    );
    assert_eq!(
        repo.job(&recent[0].id)
            .await
            .unwrap_or(None)
            .and_then(|job| job.completed_at),
        before
    );
    let latest_complete = repo
        .board_jobs(&JobFilter::default())
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|job| job.completed_at.is_some())
        .unwrap_or_else(|| panic!());
    assert_eq!(latest_complete.id, completed[10]);
}

#[tokio::test]
async fn archive_filters_apply_after_the_global_newest_ten_boundary() {
    let repo = repo().await;
    let mut completed = Vec::new();
    for index in 0..11 {
        let title = if index == 0 {
            "Needle archive"
        } else {
            "Recent completion"
        };
        let id = repo
            .save_job(None, &job_input(title))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(repo.complete_job(&id).await.unwrap_or(false));
        sqlx::query("UPDATE jobs SET completed_at=? WHERE id=?")
            .bind(format!("2026-08-20T00:00:{index:02}Z"))
            .bind(&id)
            .execute(&repo.pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        completed.push(id);
    }

    let search = "Needle".to_string();
    let stage_filtered = repo
        .board_jobs(&JobFilter {
            status: "waiting".into(),
            ..JobFilter::default()
        })
        .await
        .unwrap_or_default();
    assert_eq!(stage_filtered.len(), 10);
    assert!(stage_filtered.iter().all(|job| job.completed_at.is_some()));
    assert!(
        repo.board_jobs(&JobFilter {
            search: search.clone(),
            ..JobFilter::default()
        })
        .await
        .unwrap_or_default()
        .is_empty()
    );
    let filter = ArchiveFilter {
        search,
        completed_from: "2026-08-20".into(),
        completed_through: "2026-08-20".into(),
        category: "Print".into(),
        ..ArchiveFilter::default()
    };
    let tag = repo
        .create_tag("Archive regression", "blue")
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert!(
        repo.assign_tag(&completed[0], &tag.id)
            .await
            .unwrap_or(false)
    );
    let filter = ArchiveFilter {
        tag_id: tag.id,
        ..filter
    };
    let archive = repo
        .filtered_archived_jobs(&filter, 0, 24)
        .await
        .unwrap_or_default();
    assert_eq!(archive.len(), 1);
    assert_eq!(archive[0].id, completed[0]);
    assert_eq!(repo.archived_job_count(&filter).await.unwrap_or(0), 1);
}

#[tokio::test]
async fn job_counts_use_completed_at_as_the_authoritative_state() {
    let repo = repo().await;
    let active = repo
        .save_job(None, &job_input("Active fact"))
        .await
        .unwrap_or_default();
    let archived = repo
        .save_job(None, &job_input("Archived fact"))
        .await
        .unwrap_or_default();
    sqlx::query("UPDATE jobs SET status='complete',completed_at=NULL WHERE id=?")
        .bind(active)
        .execute(&repo.pool)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    sqlx::query("UPDATE jobs SET status='design',completed_at='2026-08-20T00:00:00Z' WHERE id=?")
        .bind(archived)
        .execute(&repo.pool)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        repo.job_counts().await.unwrap_or_default(),
        JobCounts {
            total: 2,
            active: 1
        }
    );
}

#[tokio::test]
async fn board_membership_uses_completed_at_when_status_is_inconsistent() {
    let repo = repo().await;
    let status_complete_but_active = repo
        .save_job(None, &job_input("Status says complete"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    sqlx::query("UPDATE jobs SET status='complete',completed_at=NULL WHERE id=?")
        .bind(&status_complete_but_active)
        .execute(&repo.pool)
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    let status_design_but_archived = repo
        .save_job(None, &job_input("Completed fact exists"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    sqlx::query("UPDATE jobs SET status='design',completed_at='2026-08-19T00:00:00Z' WHERE id=?")
        .bind(&status_design_but_archived)
        .execute(&repo.pool)
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    for second in 0..10 {
        let id = repo
            .save_job(None, &job_input(&format!("Recent {second}")))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(repo.complete_job(&id).await.unwrap_or(false));
        sqlx::query("UPDATE jobs SET completed_at=? WHERE id=?")
            .bind(format!("2026-08-20T00:00:{second:02}Z"))
            .bind(id)
            .execute(&repo.pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }

    let board = repo
        .board_jobs(&JobFilter::default())
        .await
        .unwrap_or_default();
    let active = board
        .iter()
        .filter(|job| job.completed_at.is_none())
        .collect::<Vec<_>>();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, status_complete_but_active);
    assert_eq!(active[0].status, "complete");

    let newest = board
        .iter()
        .filter(|job| job.completed_at.is_some())
        .collect::<Vec<_>>();
    assert_eq!(newest.len(), 10);
    assert!(newest.iter().all(|job| job.completed_at.is_some()));
    assert!(
        newest
            .iter()
            .all(|job| job.id != status_design_but_archived)
    );

    let archived = repo
        .filtered_archived_jobs(&ArchiveFilter::default(), 0, 25)
        .await
        .unwrap_or_default();
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].id, status_design_but_archived);
    assert_eq!(archived[0].status, "design");
    assert!(archived[0].completed_at.is_some());
}

#[tokio::test]
async fn completion_is_idempotent_and_reopen_obeys_pickup_guard() {
    let repo = repo().await;
    let id = repo
        .save_job(None, &job_input("Lifecycle"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(repo.complete_job(&id).await.unwrap_or(false));
    let first = repo
        .job(&id)
        .await
        .unwrap_or(None)
        .and_then(|job| job.completed_at)
        .unwrap_or_default();
    assert!(repo.complete_job(&id).await.unwrap_or(false));
    assert_eq!(
        repo.job(&id)
            .await
            .unwrap_or(None)
            .and_then(|job| job.completed_at)
            .as_deref(),
        Some(first.as_str())
    );
    assert!(!repo.reopen_job(&id, "complete").await.unwrap_or(true));
    assert!(!repo.reopen_job(&id, "unknown").await.unwrap_or(true));
    assert!(repo.reopen_job(&id, "finish").await.unwrap_or(false));
    assert_eq!(
        repo.job(&id)
            .await
            .unwrap_or(None)
            .and_then(|job| job.completed_at),
        None
    );
    assert!(repo.complete_job(&id).await.unwrap_or(false));
    assert!(repo.set_job_picked_up(&id, true).await.unwrap_or(false));
    assert!(!repo.reopen_job(&id, "design").await.unwrap_or(true));
}

#[tokio::test]
async fn tags_are_reusable_non_behavioral_metadata() {
    let repo = repo().await;
    let job = repo
        .save_job(None, &job_input("Tagged"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let tag = repo
        .create_tag("Priority", "red")
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert!(
        repo.create_tag("priority", "blue")
            .await
            .unwrap_or(None)
            .is_none()
    );
    assert!(repo.assign_tag(&job, &tag.id).await.unwrap_or(false));
    assert_eq!(
        repo.job_tag_projections(std::slice::from_ref(&job))
            .await
            .unwrap_or_default(),
        vec![JobTagProjection {
            job_id: job.clone(),
            id: tag.id.clone(),
            name: "Priority".into(),
            color_key: "red".into(),
        }]
    );
    assert!(repo.complete_job(&job).await.unwrap_or(false));
    assert_eq!(
        repo.tags_for_job(&job).await.unwrap_or_default(),
        vec![tag.clone()]
    );
    assert!(repo.archive_tag(&tag.id).await.unwrap_or(false));
    let other_job = repo
        .save_job(None, &job_input("Other"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(!repo.assign_tag(&other_job, &tag.id).await.unwrap_or(true));
    assert_eq!(repo.tags_for_job(&job).await.unwrap_or_default().len(), 1);
    assert!(repo.unassign_tag(&job, &tag.id).await.unwrap_or(false));
    assert!(repo.tags_for_job(&job).await.unwrap_or_default().is_empty());
}

#[tokio::test]
async fn tag_colors_are_limited_to_the_supported_palette() {
    let repo = repo().await;
    for color_key in TAG_COLOR_KEYS {
        let tag = repo
            .create_tag(&format!("Tag {color_key}"), color_key)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| panic!());
        assert_eq!(tag.color_key, color_key);
    }

    for (index, color_key) in ["orange", "Blue", "blue<script>", "blue-green", ""]
        .into_iter()
        .enumerate()
    {
        assert!(
            repo.create_tag(&format!("Invalid {index}"), color_key)
                .await
                .unwrap_or(None)
                .is_none()
        );
    }
}

#[tokio::test]
async fn attachment_metadata_round_trips_and_follows_job_foreign_key() {
    let repo = repo().await;
    let job = repo
        .save_job(None, &job_input("Files"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let hash = "a".repeat(64);
    let metadata = NewJobAttachment {
        original_name: "proof.pdf",
        stored_name: "1-proof.pdf",
        media_type: "application/pdf",
        byte_size: 42,
        sha256: &hash,
    };
    let attachment = repo
        .create_attachment(&job, &metadata)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert_eq!(
        repo.job_attachment_counts(std::slice::from_ref(&job))
            .await
            .unwrap_or_default(),
        vec![JobAttachmentCount {
            job_id: job.clone(),
            attachment_count: 1,
        }]
    );
    assert_eq!(
        repo.attachment(&attachment.id).await.unwrap_or(None),
        Some(attachment.clone())
    );
    assert_eq!(
        repo.attachments_for_job(&job).await.unwrap_or_default(),
        vec![attachment.clone()]
    );
    assert!(
        repo.delete_attachment(&attachment.id)
            .await
            .unwrap_or(false)
    );
    let recreated = repo
        .create_attachment(&job, &metadata)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    sqlx::query("DELETE FROM jobs WHERE id=?")
        .bind(&job)
        .execute(&repo.pool)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        repo.attachment(&recreated.id)
            .await
            .unwrap_or(None)
            .is_none()
    );
    assert!(repo.create_attachment("missing", &metadata).await.is_err());
}

#[tokio::test]
async fn meaningful_job_mutations_append_understandable_events_but_reordering_does_not() {
    let repo = repo().await;
    let original = job_input("History");
    let job = repo
        .save_job(None, &original)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    repo.save_job(Some(&job), &original)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let mut edited = original.clone();
    edited.title = "Updated history".into();
    edited.due_date = "2026-09-01".into();
    repo.save_job(Some(&job), &edited)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        repo.reorder_board_card(
            BoardLane::Production(JobStage::Design),
            &job,
            BoardReorder::Last,
        )
        .await
        .unwrap_or(false)
    );
    assert!(repo.reopen_job(&job, "print").await.unwrap_or(false));

    let tag = repo
        .create_tag("Priority", "red")
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert!(repo.assign_tag(&job, &tag.id).await.unwrap_or(false));
    assert!(repo.unassign_tag(&job, &tag.id).await.unwrap_or(false));

    let hash = "a".repeat(64);
    let attachment = repo
        .create_attachment(
            &job,
            &NewJobAttachment {
                original_name: "proof.pdf",
                stored_name: "history-proof.pdf",
                media_type: "application/pdf",
                byte_size: 42,
                sha256: &hash,
            },
        )
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    assert!(
        repo.delete_attachment(&attachment.id)
            .await
            .unwrap_or(false)
    );
    assert!(
        repo.create_or_open_job_invoice(&job)
            .await
            .unwrap_or(None)
            .is_some()
    );
    assert!(repo.complete_job(&job).await.unwrap_or(false));
    assert!(repo.reopen_job(&job, "finish").await.unwrap_or(false));
    assert!(repo.complete_job(&job).await.unwrap_or(false));
    assert!(repo.set_job_picked_up(&job, true).await.unwrap_or(false));

    let events = repo.job_events(&job).await.unwrap_or_default();
    assert_eq!(
        events.first().map(|event| event.kind.as_str()),
        Some("pickup_confirmed")
    );
    assert_eq!(
        events.last().map(|event| event.kind.as_str()),
        Some("created")
    );
    for summary in [
        "Job details updated: title, due date",
        "Moved from Design to Print",
        "Label added: Priority",
        "Label removed: Priority",
        "Attachment added: proof.pdf",
        "Attachment deleted: proof.pdf",
        "Job completed",
        "Job reopened in Finish",
        "Pickup confirmed",
    ] {
        assert!(events.iter().any(|event| event.summary == summary));
    }
    assert!(
        events
            .iter()
            .any(|event| event.summary.starts_with("Invoice INV-")
                && event.summary.ends_with(" created and linked"))
    );
    assert_eq!(
        events.len(),
        12,
        "same-lane reordering and no-op saves add no events"
    );
}

#[tokio::test]
async fn event_failure_rolls_back_the_job_mutation() {
    let repo = repo().await;
    let original = job_input("Atomic history");
    let job = repo
        .save_job(None, &original)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    sqlx::query("CREATE TRIGGER reject_detail_event BEFORE INSERT ON job_events WHEN NEW.kind='details_edited' BEGIN SELECT RAISE(ABORT, 'injected event failure'); END")
        .execute(&repo.pool)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let mut edited = original;
    edited.title = "Must roll back".into();

    assert!(repo.save_job(Some(&job), &edited).await.is_err());
    assert_eq!(
        repo.job(&job).await.unwrap_or(None).map(|job| job.title),
        Some("Atomic history".into())
    );
    assert_eq!(repo.job_events(&job).await.unwrap_or_default().len(), 1);
}

#[tokio::test]
async fn attachment_projections_are_bounded_to_jobs_and_ordered_per_job() {
    fn metadata<'a>(name: &'a str, hash: &'a str, size: i64) -> NewJobAttachment<'a> {
        NewJobAttachment {
            original_name: name,
            stored_name: name,
            media_type: "application/pdf",
            byte_size: size,
            sha256: hash,
        }
    }

    let repo = repo().await;
    let first_job = repo
        .save_job(None, &job_input("First files"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let second_job = repo
        .save_job(None, &job_input("Second files"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let omitted_job = repo
        .save_job(None, &job_input("Omitted files"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let hash = "b".repeat(64);
    let first_later = repo
        .create_attachment(&first_job, &metadata("later.pdf", &hash, 22))
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    let first_earlier = repo
        .create_attachment(&first_job, &metadata("earlier.pdf", &hash, 11))
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    let second = repo
        .create_attachment(&second_job, &metadata("second.pdf", &hash, 33))
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    let omitted = repo
        .create_attachment(&omitted_job, &metadata("omitted.pdf", &hash, 44))
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| panic!());
    for (id, created_at) in [
        (&first_later.id, "2026-08-21T12:00:02Z"),
        (&first_earlier.id, "2026-08-21T12:00:01Z"),
        (&second.id, "2026-08-21T12:00:00Z"),
        (&omitted.id, "2026-08-21T12:00:00Z"),
    ] {
        sqlx::query("UPDATE job_attachments SET created_at=? WHERE id=?")
            .bind(created_at)
            .bind(id)
            .execute(&repo.pool)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }

    let rows = repo
        .job_attachment_projections(&[first_job.clone(), second_job.clone()])
        .await
        .unwrap_or_default();
    let first_rows = rows
        .iter()
        .filter(|row| row.job_id == first_job)
        .collect::<Vec<_>>();
    assert_eq!(
        first_rows
            .iter()
            .map(|row| row.original_name.as_str())
            .collect::<Vec<_>>(),
        ["earlier.pdf", "later.pdf"]
    );
    assert_eq!(first_rows[0].id, first_earlier.id);
    assert_eq!(first_rows[0].media_type, "application/pdf");
    assert_eq!(first_rows[0].byte_size, 11);
    assert!(rows.iter().any(|row| row.id == second.id));
    assert!(rows.iter().all(|row| row.id != omitted.id));
    assert!(
        repo.job_attachment_projections(&[])
            .await
            .unwrap_or_default()
            .is_empty()
    );
}
