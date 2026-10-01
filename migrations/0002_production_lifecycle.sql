ALTER TABLE documents ADD COLUMN source_quote_id TEXT
    REFERENCES documents(id) ON UPDATE CASCADE ON DELETE RESTRICT;

CREATE UNIQUE INDEX documents_source_quote_idx
    ON documents (source_quote_id)
    WHERE source_quote_id IS NOT NULL;

CREATE TABLE jobs_lifecycle (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    title TEXT NOT NULL,
    customer_name TEXT NOT NULL,
    category TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('design', 'waiting', 'print', 'finish', 'complete')),
    due_date TEXT,
    quote_id TEXT REFERENCES documents(id) ON UPDATE CASCADE ON DELETE RESTRICT,
    invoice_id TEXT REFERENCES documents(id) ON UPDATE CASCADE ON DELETE RESTRICT,
    picked_up_at TEXT,
    notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (quote_id),
    UNIQUE (invoice_id),
    CHECK (quote_id IS NULL OR quote_id <> invoice_id)
);

INSERT INTO jobs_lifecycle (
    id, title, customer_name, category, status, due_date, quote_id, invoice_id,
    picked_up_at, notes, created_at, updated_at
)
SELECT
    id, title, customer_name, category,
    CASE status
        WHEN 'queued' THEN 'design'
        WHEN 'in_progress' THEN 'print'
        WHEN 'ready' THEN 'complete'
        WHEN 'done' THEN 'complete'
    END,
    due_date,
    CASE WHEN quote_id IS NULL OR NOT EXISTS (
        SELECT 1 FROM jobs AS earlier
        WHERE earlier.quote_id = jobs.quote_id
          AND (earlier.created_at < jobs.created_at
               OR (earlier.created_at = jobs.created_at AND earlier.id < jobs.id))
    ) THEN quote_id END,
    CASE WHEN invoice_id IS NULL OR NOT EXISTS (
        SELECT 1 FROM jobs AS earlier
        WHERE earlier.invoice_id = jobs.invoice_id
          AND (earlier.created_at < jobs.created_at
               OR (earlier.created_at = jobs.created_at AND earlier.id < jobs.id))
    ) THEN invoice_id END,
    NULL, notes, created_at, updated_at
FROM jobs;

DROP TABLE jobs;
ALTER TABLE jobs_lifecycle RENAME TO jobs;

CREATE INDEX jobs_active_idx ON jobs (status, due_date);

CREATE TRIGGER documents_source_quote_kind_insert
BEFORE INSERT ON documents
WHEN NEW.source_quote_id IS NOT NULL
BEGIN
    SELECT CASE WHEN NEW.kind <> 'invoice'
        OR NOT EXISTS (
            SELECT 1 FROM documents
            WHERE id = NEW.source_quote_id AND kind = 'quote'
        )
    THEN RAISE(ABORT, 'source_quote_id must link an invoice to a quote') END;
END;

CREATE TRIGGER documents_source_quote_kind_update
BEFORE UPDATE OF source_quote_id, kind ON documents
WHEN NEW.source_quote_id IS NOT NULL
BEGIN
    SELECT CASE WHEN NEW.kind <> 'invoice'
        OR NOT EXISTS (
            SELECT 1 FROM documents
            WHERE id = NEW.source_quote_id AND kind = 'quote'
        )
    THEN RAISE(ABORT, 'source_quote_id must link an invoice to a quote') END;
END;

CREATE TRIGGER jobs_document_kinds_insert
BEFORE INSERT ON jobs
BEGIN
    SELECT CASE WHEN NEW.quote_id IS NOT NULL
        AND NOT EXISTS (
            SELECT 1 FROM documents WHERE id = NEW.quote_id AND kind = 'quote'
        )
    THEN RAISE(ABORT, 'quote_id must reference a quote') END;
    SELECT CASE WHEN NEW.invoice_id IS NOT NULL
        AND NOT EXISTS (
            SELECT 1 FROM documents WHERE id = NEW.invoice_id AND kind = 'invoice'
        )
    THEN RAISE(ABORT, 'invoice_id must reference an invoice') END;
END;

CREATE TRIGGER jobs_document_kinds_update
BEFORE UPDATE OF quote_id, invoice_id ON jobs
BEGIN
    SELECT CASE WHEN NEW.quote_id IS NOT NULL
        AND NOT EXISTS (
            SELECT 1 FROM documents WHERE id = NEW.quote_id AND kind = 'quote'
        )
    THEN RAISE(ABORT, 'quote_id must reference a quote') END;
    SELECT CASE WHEN NEW.invoice_id IS NOT NULL
        AND NOT EXISTS (
            SELECT 1 FROM documents WHERE id = NEW.invoice_id AND kind = 'invoice'
        )
    THEN RAISE(ABORT, 'invoice_id must reference an invoice') END;
END;
