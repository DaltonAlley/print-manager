ALTER TABLE documents ADD COLUMN quote_board_position INTEGER
    CHECK (quote_board_position IS NULL OR quote_board_position >= 0);

ALTER TABLE jobs ADD COLUMN active_board_position INTEGER
    CHECK (active_board_position IS NULL OR active_board_position >= 0);

ALTER TABLE jobs ADD COLUMN complete_board_position INTEGER
    CHECK (complete_board_position IS NULL OR complete_board_position >= 0);

CREATE INDEX documents_quote_board_position_idx
    ON documents (quote_board_position, updated_at DESC, id DESC)
    WHERE kind = 'quote' AND status <> 'declined';

CREATE INDEX jobs_active_board_position_idx
    ON jobs (active_board_position, due_date, updated_at DESC, id DESC)
    WHERE completed_at IS NULL;

CREATE INDEX jobs_complete_board_position_idx
    ON jobs (complete_board_position, completed_at DESC, id DESC)
    WHERE completed_at IS NOT NULL;
