ALTER TABLE settings ADD COLUMN default_turnaround_days INTEGER NOT NULL DEFAULT 3
    CHECK (default_turnaround_days BETWEEN 1 AND 365);

UPDATE jobs
SET due_date = date(created_at, '+3 days')
WHERE due_date IS NULL;
