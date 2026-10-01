ALTER TABLE jobs ADD COLUMN completed_at TEXT;

UPDATE jobs
SET completed_at = CASE
    WHEN trim(updated_at) <> '' THEN updated_at
    ELSE '1970-01-01T00:00:00Z'
END
WHERE status = 'complete';

CREATE INDEX jobs_completed_idx ON jobs (completed_at DESC, id DESC)
    WHERE completed_at IS NOT NULL;

CREATE TABLE tags (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 100),
    color_key TEXT NOT NULL CHECK (
        color_key IN ('neutral', 'blue', 'green', 'amber', 'red', 'purple')
    ),
    archived_at TEXT,
    created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX tags_active_name_idx
    ON tags (name COLLATE NOCASE)
    WHERE archived_at IS NULL;

CREATE TABLE job_tags (
    job_id TEXT NOT NULL REFERENCES jobs(id) ON UPDATE CASCADE ON DELETE CASCADE,
    tag_id TEXT NOT NULL REFERENCES tags(id) ON UPDATE CASCADE ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    PRIMARY KEY (job_id, tag_id)
);

CREATE INDEX job_tags_tag_idx ON job_tags (tag_id, job_id);

CREATE TABLE job_attachments (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    job_id TEXT NOT NULL REFERENCES jobs(id) ON UPDATE CASCADE ON DELETE CASCADE,
    original_name TEXT NOT NULL CHECK (length(trim(original_name)) BETWEEN 1 AND 255),
    stored_name TEXT NOT NULL CHECK (length(trim(stored_name)) BETWEEN 1 AND 255),
    media_type TEXT NOT NULL CHECK (length(trim(media_type)) BETWEEN 1 AND 255),
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    created_at TEXT NOT NULL,
    UNIQUE (job_id, stored_name)
);

CREATE INDEX job_attachments_job_created_idx
    ON job_attachments (job_id, created_at, id);
