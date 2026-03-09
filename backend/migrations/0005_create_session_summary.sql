-- session_summary: 会話の古いターンをまとめたサマリテーブル
CREATE TABLE IF NOT EXISTS session_summary (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id       VARCHAR NOT NULL,
    summary_id       INTEGER NOT NULL,
    start_message_id INTEGER NOT NULL,
    end_message_id   INTEGER NOT NULL,
    summary          VARCHAR NOT NULL,
    updated_by       VARCHAR NOT NULL,
    last_updated     DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(session_id, summary_id)
);

CREATE INDEX IF NOT EXISTS idx_session_summary_session_id ON session_summary(session_id);
