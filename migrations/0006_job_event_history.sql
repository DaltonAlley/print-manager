CREATE TABLE job_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL REFERENCES jobs(id) ON UPDATE CASCADE ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN (
        'created', 'details_edited', 'stage_changed', 'completed', 'reopened',
        'pickup_confirmed', 'pickup_reopened', 'invoice_created', 'invoice_linked',
        'attachment_added', 'attachment_deleted', 'label_added', 'label_removed'
    )),
    summary TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    occurred_at TEXT NOT NULL
);

CREATE INDEX job_events_job_newest_idx
    ON job_events (job_id, occurred_at DESC, id DESC);

INSERT INTO job_events (job_id, kind, summary, occurred_at)
SELECT id, 'created', 'Job created',
       CASE WHEN trim(created_at) <> '' THEN created_at ELSE '1970-01-01T00:00:00Z' END
FROM jobs
ORDER BY CASE WHEN trim(created_at) <> '' THEN created_at ELSE '1970-01-01T00:00:00Z' END, id;

INSERT INTO job_events (job_id, kind, summary, occurred_at)
SELECT id, 'completed', 'Job completed', completed_at
FROM jobs
WHERE completed_at IS NOT NULL
ORDER BY completed_at, id;

INSERT INTO job_events (job_id, kind, summary, occurred_at)
SELECT id, 'pickup_confirmed', 'Pickup confirmed', picked_up_at
FROM jobs
WHERE picked_up_at IS NOT NULL
ORDER BY picked_up_at, id;
