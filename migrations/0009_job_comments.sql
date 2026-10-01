ALTER TABLE job_events RENAME TO job_events_without_comments;

CREATE TABLE job_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL REFERENCES jobs(id) ON UPDATE CASCADE ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN (
        'created', 'details_edited', 'stage_changed', 'completed', 'reopened',
        'pickup_confirmed', 'pickup_reopened', 'invoice_created', 'invoice_linked',
        'attachment_added', 'attachment_deleted', 'label_added', 'label_removed',
        'comment'
    )),
    summary TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    occurred_at TEXT NOT NULL
);

INSERT INTO job_events (id, job_id, kind, summary, occurred_at)
SELECT id, job_id, kind, summary, occurred_at
FROM job_events_without_comments;

DROP TABLE job_events_without_comments;

CREATE INDEX job_events_job_newest_idx
    ON job_events (job_id, occurred_at DESC, id DESC);
