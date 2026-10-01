CREATE TABLE settings (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    singleton INTEGER NOT NULL DEFAULT 1 UNIQUE CHECK (singleton = 1),
    business_name TEXT NOT NULL,
    address TEXT NOT NULL DEFAULT '',
    phone TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL DEFAULT '',
    logo_path TEXT,
    default_tax_basis_points INTEGER NOT NULL DEFAULT 0 CHECK (default_tax_basis_points BETWEEN 0 AND 10000),
    quote_prefix TEXT NOT NULL DEFAULT 'Q-',
    invoice_prefix TEXT NOT NULL DEFAULT 'INV-',
    next_quote_number INTEGER NOT NULL DEFAULT 1 CHECK (next_quote_number > 0),
    next_invoice_number INTEGER NOT NULL DEFAULT 1 CHECK (next_invoice_number > 0)
);

CREATE TABLE documents (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    kind TEXT NOT NULL CHECK (kind IN ('quote', 'invoice')),
    number TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        (kind = 'quote' AND status IN ('draft', 'sent', 'accepted', 'declined')) OR
        (kind = 'invoice' AND status IN ('draft', 'sent', 'paid'))
    ),
    customer_name TEXT NOT NULL,
    customer_email TEXT NOT NULL DEFAULT '',
    customer_phone TEXT NOT NULL DEFAULT '',
    billing_address TEXT NOT NULL DEFAULT '',
    issue_date TEXT NOT NULL,
    expiry_or_due_date TEXT,
    discount_cents INTEGER NOT NULL DEFAULT 0 CHECK (discount_cents >= 0),
    tax_basis_points INTEGER NOT NULL DEFAULT 0 CHECK (tax_basis_points BETWEEN 0 AND 10000),
    notes TEXT NOT NULL DEFAULT '',
    terms TEXT NOT NULL DEFAULT '',
    last_emailed_at TEXT,
    resend_message_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (kind, number)
);

CREATE TABLE document_lines (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    document_id TEXT NOT NULL REFERENCES documents(id) ON UPDATE CASCADE ON DELETE RESTRICT,
    display_position INTEGER NOT NULL CHECK (display_position >= 0),
    description TEXT NOT NULL,
    quantity TEXT NOT NULL,
    unit_label TEXT NOT NULL DEFAULT '',
    unit_price_cents INTEGER NOT NULL CHECK (unit_price_cents >= 0),
    UNIQUE (document_id, display_position)
);

CREATE TABLE jobs (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36),
    title TEXT NOT NULL,
    customer_name TEXT NOT NULL,
    category TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued', 'in_progress', 'ready', 'done')),
    due_date TEXT,
    quote_id TEXT REFERENCES documents(id) ON UPDATE CASCADE ON DELETE RESTRICT,
    invoice_id TEXT REFERENCES documents(id) ON UPDATE CASCADE ON DELETE RESTRICT,
    notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (quote_id IS NULL OR quote_id <> invoice_id)
);

CREATE INDEX document_lines_document_position_idx
    ON document_lines (document_id, display_position);
CREATE INDEX jobs_active_idx ON jobs (status, due_date);
