ALTER TABLE jobs ADD COLUMN design_board_position INTEGER
    CHECK (design_board_position IS NULL OR design_board_position >= 0);

ALTER TABLE jobs ADD COLUMN waiting_board_position INTEGER
    CHECK (waiting_board_position IS NULL OR waiting_board_position >= 0);

ALTER TABLE jobs ADD COLUMN print_board_position INTEGER
    CHECK (print_board_position IS NULL OR print_board_position >= 0);

ALTER TABLE jobs ADD COLUMN finish_board_position INTEGER
    CHECK (finish_board_position IS NULL OR finish_board_position >= 0);

UPDATE jobs SET design_board_position = active_board_position WHERE status = 'design';
UPDATE jobs SET waiting_board_position = active_board_position WHERE status = 'waiting';
UPDATE jobs SET print_board_position = active_board_position WHERE status = 'print';
UPDATE jobs SET finish_board_position = active_board_position WHERE status = 'finish';

CREATE INDEX jobs_design_board_position_idx
    ON jobs (design_board_position, due_date, updated_at DESC, id DESC)
    WHERE status = 'design' AND completed_at IS NULL;

CREATE INDEX jobs_waiting_board_position_idx
    ON jobs (waiting_board_position, due_date, updated_at DESC, id DESC)
    WHERE status = 'waiting' AND completed_at IS NULL;

CREATE INDEX jobs_print_board_position_idx
    ON jobs (print_board_position, due_date, updated_at DESC, id DESC)
    WHERE status = 'print' AND completed_at IS NULL;

CREATE INDEX jobs_finish_board_position_idx
    ON jobs (finish_board_position, due_date, updated_at DESC, id DESC)
    WHERE status = 'finish' AND completed_at IS NULL;
